use crate::analytics;
use crate::device_registry::{self, RegistryError};
use crate::models::{
    AnalyticsQuery, DeviceSearchQuery, DeviceSearchResponse,
    PaymentRequest, PaymentResponse, Session, HeartbeatRequest, TelemetryUploadRequest,
    Review, ReviewRequest, DeviceListQuery, DeviceRegistrationRequest, DeviceRegistrationResponse,
    DeviceUpdateRequest, ManagedDevice, PaymentHistoryQuery, QrScanRequest, QrScanAnalytics,
    OwnerEarningsQuery, OwnerEarningsResponse, OwnerDeviceStatus, WithdrawalRequest,
    WithdrawalResponse,
};
use crate::services;
use axum::{
    extract::{Path, Query, ws::{WebSocketUpgrade, WebSocket, Message}},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

/// Search and filter devices.
pub async fn search_devices(Query(query): Query<DeviceSearchQuery>) -> Json<DeviceSearchResponse> {
    Json(services::search_devices(&query))
}

/// Process payment and grant access with Stellar verification.
pub async fn process_payment(
    Json(payment): Json<PaymentRequest>,
) -> Result<Json<PaymentResponse>, StatusCode> {
    // 1. Verify payment on Stellar
    match services::verify_payment(&payment.tx_hash, &payment.device_id, &payment.user_address)
        .await
    {
        Ok(true) => {
            // Payment verified - grant access and store session in global store
            let session = services::create_session(payment.device_id, payment.user_address);
            // Record the payment so it appears in the user's transaction history.
            services::record_payment(&payment.tx_hash, payment.amount, &session);
            Ok(Json(PaymentResponse {
                access_granted: true,
                session_id: session.id,
                expires_at: session.expires_at.to_rfc3339(),
            }))
        }
        Ok(false) => {
            // Replay attack detected
            Err(StatusCode::CONFLICT)
        }
        Err(msg) => {
            // Verification failed
            eprintln!("Payment verification failed: {}", msg);
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

/// Get session details.
pub async fn get_session(Path(id): Path<String>) -> Result<Json<Session>, StatusCode> {
    match services::get_session(&id) {
        Some(session) => Ok(Json(session)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Debug, Deserialize)]
pub struct SessionsQuery {
    pub user: String,
}

/// Get sessions by user.
pub async fn get_sessions(Query(query): Query<SessionsQuery>) -> Json<Vec<Session>> {
    Json(services::get_sessions_by_user(&query.user))
}

/// Extend session.
pub async fn extend_session(
    Path(id): Path<String>,
    Json(payment): Json<PaymentRequest>,
) -> Result<Json<PaymentResponse>, StatusCode> {
    match services::verify_payment(&payment.tx_hash, &payment.device_id, &payment.user_address)
        .await
    {
        Ok(true) => {
            match services::extend_session(&id, 1) {
                Ok(session) => {
                    // Record the extension as a payment in the user's history.
                    services::record_payment(&payment.tx_hash, payment.amount, &session);
                    Ok(Json(PaymentResponse {
                        access_granted: true,
                        session_id: session.id,
                        expires_at: session.expires_at.to_rfc3339(),
                    }))
                }
                Err(_) => Err(StatusCode::NOT_FOUND),
            }
        }
        Ok(false) => Err(StatusCode::CONFLICT),
        Err(_) => Err(StatusCode::BAD_REQUEST),
    }
}

/// End session.
pub async fn end_session(Path(id): Path<String>) -> Result<StatusCode, StatusCode> {
    match services::end_session(&id) {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

/// Upgrade HTTP GET request to WebSocket.
pub async fn telemetry_ws(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_telemetry_socket(socket, id))
}

/// Send live telemetry data packets to the client.
async fn handle_telemetry_socket(mut socket: WebSocket, session_id: String) {
    // Verify session exists
    let session = match services::get_session(&session_id) {
        Some(s) => s,
        None => {
            let _ = socket.send(Message::Text("Session not found".to_string())).await;
            return;
        }
    };

    let mut rx = services::subscribe_telemetry(&session.device_id);

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(data) => {
                        // Check if session remains active
                        let current_session = match services::get_session(&session_id) {
                            Some(s) => s,
                            None => break,
                        };
                        let now = chrono::Utc::now();
                        if !current_session.active || current_session.expires_at < now {
                            let _ = socket.send(Message::Text(serde_json::json!({
                                "error": "Session expired or terminated",
                                "active": false
                            }).to_string())).await;
                            break;
                        }

                        if let Ok(msg_text) = serde_json::to_string(&data) {
                            if socket.send(Message::Text(msg_text)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(_) => {
                        // Lagged or closed
                        continue;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

/// `GET /devices/:id/analytics`
///
/// Query params:
/// - `period`   – daily | weekly | monthly  (default: daily)
/// - `lookback` – number of periods to include (default: 30/12/12)
/// - `format`   – json | csv  (default: json)
///
/// Returns a full analytics report for the device owner.
/// Use `format=csv` for an exportable spreadsheet.
pub async fn get_device_analytics(
    Path(id): Path<String>,
    Query(query): Query<AnalyticsQuery>,
) -> Response {
    let report = match analytics::generate_report(&id, &query) {
        Some(r) => r,
        None => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Device not found" })))
                .into_response();
        }
    };

    let want_csv = query
        .format
        .as_deref()
        .map(|f| f.eq_ignore_ascii_case("csv"))
        .unwrap_or(false);

    if want_csv {
        match analytics::report_to_csv(&report) {
            Ok(csv) => (
                StatusCode::OK,
                [
                    (
                        header::CONTENT_TYPE,
                        "text/csv; charset=utf-8",
                    ),
                    (
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=\"analytics.csv\"",
                    ),
                ],
                csv,
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response(),
        }
    } else {
        Json(report).into_response()
    }
}

/// Process device heartbeat.
pub async fn device_heartbeat(
    Path(id): Path<String>,
    Json(payload): Json<HeartbeatRequest>,
) -> Result<StatusCode, StatusCode> {
    match services::record_heartbeat(&id, payload.health_metrics) {
        Ok(_) => Ok(StatusCode::OK),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

/// Process telemetry data ingestion.
pub async fn upload_telemetry(
    Path(id): Path<String>,
    Json(payload): Json<TelemetryUploadRequest>,
) -> Result<StatusCode, StatusCode> {
    let _session = match services::get_session(&payload.session_id) {
        Some(s) if s.active && s.device_id == id => s,
        _ => return Err(StatusCode::UNAUTHORIZED),
    };

    services::ingest_telemetry(&id, payload.data);
    Ok(StatusCode::OK)
}

pub async fn add_device_review(
    Path(id): Path<String>,
    Json(req): Json<ReviewRequest>,
) -> Result<Json<Review>, (StatusCode, String)> {
    match services::add_review(&id, req) {
        Ok(review) => Ok(Json(review)),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

pub async fn get_device_reviews(
    Path(id): Path<String>,
) -> Json<Vec<Review>> {
    Json(services::get_reviews(&id))
}

// ─── Device Registration & Management (CRUD) ───────────────────────────────────

/// Map a [`RegistryError`] onto an HTTP status + message.
fn registry_error_response(err: RegistryError) -> (StatusCode, String) {
    match err {
        RegistryError::NotFound => (StatusCode::NOT_FOUND, "Device not found".to_string()),
        RegistryError::Unauthorized => (
            StatusCode::UNAUTHORIZED,
            "Missing X-Owner-Address header".to_string(),
        ),
        RegistryError::Forbidden => (
            StatusCode::FORBIDDEN,
            "You are not the owner of this device".to_string(),
        ),
        RegistryError::Validation(msg) => (StatusCode::BAD_REQUEST, msg),
    }
}

/// Extract the owner Stellar address from the `X-Owner-Address` header.
fn owner_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-owner-address")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// `POST /devices` — register a new device.
pub async fn register_device(
    Json(req): Json<DeviceRegistrationRequest>,
) -> Result<(StatusCode, Json<DeviceRegistrationResponse>), (StatusCode, String)> {
    match device_registry::register(req) {
        Ok(device) => Ok((
            StatusCode::CREATED,
            Json(DeviceRegistrationResponse {
                id: device.id,
                name: device.name,
                message: "Device registered successfully".to_string(),
            }),
        )),
        Err(e) => Err(registry_error_response(e)),
    }
}

/// `GET /devices` — list managed devices with filters and pagination.
///
/// Returns a JSON array (backwards-compatible) and exposes pagination metadata
/// via the `X-Total-Count`, `X-Limit` and `X-Offset` response headers.
pub async fn list_devices(Query(query): Query<DeviceListQuery>) -> Response {
    let result = device_registry::list(&query);
    (
        StatusCode::OK,
        [
            ("x-total-count", result.total.to_string()),
            ("x-limit", result.limit.to_string()),
            ("x-offset", result.offset.to_string()),
        ],
        Json(result.data),
    )
        .into_response()
}

/// `GET /devices/:id` — device details.
pub async fn get_managed_device(
    Path(id): Path<String>,
) -> Result<Json<ManagedDevice>, (StatusCode, String)> {
    match device_registry::get(&id) {
        Some(device) => Ok(Json(device)),
        None => Err((StatusCode::NOT_FOUND, "Device not found".to_string())),
    }
}

/// `PUT /devices/:id` — update device info (owner-authenticated).
pub async fn update_device(
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<DeviceUpdateRequest>,
) -> Result<Json<ManagedDevice>, (StatusCode, String)> {
    let owner = owner_from_headers(&headers);
    match device_registry::update(&id, owner.as_deref(), req) {
        Ok(device) => Ok(Json(device)),
        Err(e) => Err(registry_error_response(e)),
    }
}

/// `DELETE /devices/:id` — deregister a device (owner-authenticated).
pub async fn delete_device(
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, String)> {
    let owner = owner_from_headers(&headers);
    match device_registry::delete(&id, owner.as_deref()) {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err(registry_error_response(e)),
    }
}

// ─── Payment History ───────────────────────────────────────────────────────────

/// `GET /payments` — paginated, filterable payment history for a user.
///
/// Query params: `user` (required), `device_id`, `status`, `from`, `to`,
/// `format` (`json` | `csv`).
pub async fn get_payment_history(Query(query): Query<PaymentHistoryQuery>) -> Response {
    let report = services::get_payment_history(&query);

    let want_csv = query
        .format
        .as_deref()
        .map(|f| f.eq_ignore_ascii_case("csv"))
        .unwrap_or(false);

    if want_csv {
        match services::payment_history_to_csv(&report) {
            Ok(csv) => (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
                    (
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=\"payment-history.csv\"",
                    ),
                ],
                csv,
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response(),
        }
    } else {
        Json(report).into_response()
    }
}

// ─── QR Code Scan Analytics ────────────────────────────────────────────────────

/// `POST /devices/:id/qr-scan` — record a QR-code scan.
///
/// The optional `source` query param tags the scan (e.g. a print batch).
pub async fn record_qr_scan(
    Path(id): Path<String>,
    Query(req): Query<QrScanRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    match services::record_qr_scan(&id, req.source) {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((StatusCode::NOT_FOUND, e)),
    }
}

/// `GET /devices/:id/qr-analytics` — QR-scan analytics for a device.
pub async fn get_qr_analytics(Path(id): Path<String>) -> Json<QrScanAnalytics> {
    Json(services::get_qr_analytics(&id))
}

// ─── Earnings / Owner Dashboard ─────────────────────────────────────────────────

/// `GET /earnings` — aggregate earnings summary and time-series for a device owner.
///
/// Query params: `owner_address` (required), `period` (`daily` | `weekly` | `monthly`),
/// `lookback` (number of periods).
pub async fn get_owner_earnings(
    Query(query): Query<OwnerEarningsQuery>,
) -> Json<OwnerEarningsResponse> {
    Json(analytics::generate_owner_report(&query))
}

#[derive(Debug, Deserialize)]
pub struct OwnerDevicesQuery {
    pub owner_address: String,
}

/// `GET /earnings/devices` — per-device status and earnings for the owner's device list.
pub async fn get_owner_devices(
    Query(query): Query<OwnerDevicesQuery>,
) -> Json<Vec<OwnerDeviceStatus>> {
    Json(analytics::get_owner_device_statuses(&query.owner_address))
}

/// `POST /earnings/withdraw` — withdraw accumulated earnings to a destination address.
pub async fn withdraw_earnings(
    Json(req): Json<WithdrawalRequest>,
) -> Result<Json<WithdrawalResponse>, (StatusCode, String)> {
    match services::process_withdrawal(&req.owner_address, req.amount, &req.destination_address) {
        Ok((tx_hash, fee)) => Ok(Json(WithdrawalResponse {
            success: true,
            tx_hash,
            amount: req.amount,
            fee,
            message: "Withdrawal submitted".to_string(),
        })),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}
