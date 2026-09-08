use crate::models::{
    Device, DeviceCategory, DeviceSearchQuery, DeviceSearchResponse, SortField, SortOrder, Session, TelemetryData, DeviceStatus, Review, ReviewRequest,
    PaymentRecord, PaymentHistoryQuery, PaymentHistoryResponse, PaymentHistoryEntry,
    QrScanAnalytics, QrScanDailyPoint,
};
use chrono::{DateTime, Utc};
use crate::stellar_service::StellarService;
use lazy_static::lazy_static;
use std::sync::Arc;
use tokio::sync::broadcast;

lazy_static! {
    static ref STELLAR_SERVICE: Arc<StellarService> = Arc::new(StellarService::new());
}

/// Haversine distance between two WGS-84 coordinates, returns kilometres.
fn haversine_km(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlng = (lng2 - lng1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlng / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    EARTH_RADIUS_KM * c
}

/// Master device catalogue.  In a production system this would be a database
/// query; here we keep a rich in-memory dataset so every filter/sort path can
/// be exercised in tests.
pub fn get_mock_devices() -> Vec<Device> {
    vec![
        Device {
            id: "device-001".to_string(),
            name: "Smart Lock Alpha".to_string(),
            description: "High-security smart lock for residential use".to_string(),
            price: 5.0,
            available: true,
            location: "Building A, Floor 3".to_string(),
            category: DeviceCategory::Access,
            rating: 4.5,
            popularity: 320,
            latitude: 37.7749,
            longitude: -122.4194,
            owner_address: "GBV".to_string(),
        },
        Device {
            id: "device-002".to_string(),
            name: "Temperature Sensor".to_string(),
            description: "Industrial-grade temperature monitoring sensor".to_string(),
            price: 2.5,
            available: true,
            location: "Warehouse B".to_string(),
            category: DeviceCategory::Environmental,
            rating: 4.2,
            popularity: 510,
            latitude: 37.7751,
            longitude: -122.4180,
            owner_address: "GBV".to_string(),
        },
        Device {
            id: "device-003".to_string(),
            name: "Security Camera".to_string(),
            description: "4K security camera with night vision".to_string(),
            price: 10.0,
            available: true,
            location: "Parking Lot C".to_string(),
            category: DeviceCategory::Security,
            rating: 4.8,
            popularity: 890,
            latitude: 37.7760,
            longitude: -122.4200,
            owner_address: "GBV".to_string(),
        },
        Device {
            id: "device-004".to_string(),
            name: "Air Quality Monitor".to_string(),
            description: "Real-time air quality monitoring device".to_string(),
            price: 3.0,
            available: false,
            location: "Office D".to_string(),
            category: DeviceCategory::Environmental,
            rating: 3.9,
            popularity: 210,
            latitude: 37.7730,
            longitude: -122.4210,
            owner_address: "GA2".to_string(),
        },
        Device {
            id: "device-005".to_string(),
            name: "Smart Thermostat".to_string(),
            description: "Energy-efficient climate control system".to_string(),
            price: 7.5,
            available: true,
            location: "Building E, Floor 1".to_string(),
            category: DeviceCategory::Climate,
            rating: 4.6,
            popularity: 640,
            latitude: 37.7740,
            longitude: -122.4170,
            owner_address: "GBV".to_string(),
        },
        Device {
            id: "device-006".to_string(),
            name: "Water Flow Sensor".to_string(),
            description: "Monitor water usage and detect leaks".to_string(),
            price: 4.0,
            available: true,
            location: "Utility Room F".to_string(),
            category: DeviceCategory::Utility,
            rating: 4.1,
            popularity: 175,
            latitude: 37.7755,
            longitude: -122.4165,
            owner_address: "GBV".to_string(),
        },
        Device {
            id: "device-007".to_string(),
            name: "Motion Detector Pro".to_string(),
            description: "Wide-angle PIR motion detector with tamper alert".to_string(),
            price: 6.0,
            available: true,
            location: "Lobby G".to_string(),
            category: DeviceCategory::Security,
            rating: 4.3,
            popularity: 430,
            latitude: 37.7745,
            longitude: -122.4155,
            owner_address: "GBV".to_string(),
        },
        Device {
            id: "device-008".to_string(),
            name: "Humidity Sensor".to_string(),
            description: "Precision humidity and dew-point sensor for clean rooms".to_string(),
            price: 1.5,
            available: true,
            location: "Lab H".to_string(),
            category: DeviceCategory::Environmental,
            rating: 3.7,
            popularity: 88,
            latitude: 37.7770,
            longitude: -122.4220,
            owner_address: "GA2".to_string(),
        },
        Device {
            id: "device-009".to_string(),
            name: "Smart Door Bell".to_string(),
            description: "Video doorbell with two-way audio and face recognition".to_string(),
            price: 12.0,
            available: false,
            location: "Entrance I".to_string(),
            category: DeviceCategory::Access,
            rating: 4.7,
            popularity: 755,
            latitude: 37.7735,
            longitude: -122.4185,
            owner_address: "GBV".to_string(),
        },
        Device {
            id: "device-010".to_string(),
            name: "CO2 Monitor".to_string(),
            description: "NDIR CO2 sensor for ventilation and occupancy analytics".to_string(),
            price: 3.5,
            available: true,
            location: "Conference Room J".to_string(),
            category: DeviceCategory::Environmental,
            rating: 4.0,
            popularity: 300,
            latitude: 37.7762,
            longitude: -122.4195,
            owner_address: "GBV".to_string(),
        },
    ]
}

/// Apply full-text search, filters, geospatial proximity, sorting, and
/// cursor-based pagination to the device catalogue.
pub fn search_devices(query: &DeviceSearchQuery) -> DeviceSearchResponse {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let mut all_devices = get_mock_devices();
    enrich_devices_with_ratings(&mut all_devices);

    // ── 1. Full-text filter ───────────────────────────────────────────────
    let needle = query.q.as_deref().unwrap_or("").to_lowercase();

    // ── 2. Apply all filters ──────────────────────────────────────────────
    let mut filtered: Vec<Device> = all_devices
        .into_iter()
        .filter(|d| {
            // Full-text: name or description contains the search term.
            if !needle.is_empty()
                && !d.name.to_lowercase().contains(&needle)
                && !d.description.to_lowercase().contains(&needle)
            {
                return false;
            }
            // Category
            if let Some(ref cat) = query.category {
                if &d.category != cat {
                    return false;
                }
            }
            // Availability
            if let Some(avail) = query.available {
                if d.available != avail {
                    return false;
                }
            }
            // Price range
            if let Some(min) = query.min_price {
                if d.price < min {
                    return false;
                }
            }
            if let Some(max) = query.max_price {
                if d.price > max {
                    return false;
                }
            }
            // Minimum rating
            if let Some(min_r) = query.min_rating {
                if d.rating < min_r {
                    return false;
                }
            }
            // Geospatial proximity
            if let (Some(lat), Some(lng), Some(radius)) = (query.lat, query.lng, query.radius_km) {
                let dist = haversine_km(lat, lng, d.latitude, d.longitude);
                if dist > radius {
                    return false;
                }
            }
            true
        })
        .collect();

    // ── 3. Sort ───────────────────────────────────────────────────────────
    // Always sort ascending first, then reverse for descending order.
    match &query.sort_by {
        Some(SortField::Price) => {
            filtered.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
        }
        Some(SortField::Rating) => {
            filtered.sort_by(|a, b| a.rating.partial_cmp(&b.rating).unwrap());
        }
        Some(SortField::Popularity) => {
            filtered.sort_by_key(|a| a.popularity);
        }
        None => {
            // Default: stable insertion order (by id lexicographic).
            filtered.sort_by(|a, b| a.id.cmp(&b.id));
        }
    }
    if query.sort_order == SortOrder::Desc {
        filtered.reverse();
    }

    let total = filtered.len();

    // ── 4. Cursor pagination ──────────────────────────────────────────────
    // The cursor is the id of the last device on the previous page.
    let start_index = if let Some(ref cursor) = query.cursor {
        filtered
            .iter()
            .position(|d| &d.id == cursor)
            .map(|pos| pos + 1) // start *after* the cursor device
            .unwrap_or(0)
    } else {
        0
    };

    let page: Vec<Device> = filtered.into_iter().skip(start_index).take(limit).collect();

    let next_cursor = if start_index + page.len() < total {
        page.last().map(|d| d.id.clone())
    } else {
        None
    };

    DeviceSearchResponse {
        total,
        limit,
        next_cursor,
        data: page,
    }
}

/// Verify a payment against Stellar Horizon.
///
/// Checks:
/// 1. Device exists
/// 2. Transaction exists and is successful on Stellar
/// 3. Amount matches device price
/// 4. Destination matches device owner wallet
/// 5. Prevents replay attacks via transaction hash deduplication
pub async fn verify_payment(
    tx_hash: &str,
    device_id: &str,
    user_address: &str,
) -> Result<bool, String> {
    let device = get_mock_devices()
        .into_iter()
        .find(|d| d.id == device_id)
        .ok_or_else(|| "Device not found".to_string())?;

    STELLAR_SERVICE
        .verify_payment(tx_hash, device.price, user_address)
        .await
}

lazy_static! {
    pub static ref DEVICE_STATUSES: std::sync::RwLock<std::collections::HashMap<String, DeviceStatus>> =
        std::sync::RwLock::new(std::collections::HashMap::new());
}

pub fn record_heartbeat(device_id: &str, health_metrics: Option<serde_json::Value>) -> Result<(), String> {
    let devices = get_mock_devices();
    if !devices.iter().any(|d| d.id == device_id) {
        return Err("Device not found".to_string());
    }

    let mut statuses = DEVICE_STATUSES.write().unwrap();
    let status = statuses.entry(device_id.to_string()).or_insert_with(|| DeviceStatus {
        device_id: device_id.to_string(),
        online: true,
        last_seen: Some(chrono::Utc::now()),
        missed_heartbeats: 0,
        health_metrics: health_metrics.clone(),
    });

    status.online = true;
    status.last_seen = Some(chrono::Utc::now());
    status.missed_heartbeats = 0;
    status.health_metrics = health_metrics;

    Ok(())
}

pub fn check_offline_devices() {
    let mut statuses = DEVICE_STATUSES.write().unwrap();
    let now = chrono::Utc::now();
    for (id, status) in statuses.iter_mut() {
        if let Some(last_seen) = status.last_seen {
            let duration = now.signed_duration_since(last_seen);
            let expected_heartbeats_missed = duration.num_seconds() / 60;
            if expected_heartbeats_missed >= 3 && status.online {
                status.online = false;
                status.missed_heartbeats = expected_heartbeats_missed as u32;
                println!("Notification: Device owner of {} notified - Device is offline! Missed heartbeats: {}", id, status.missed_heartbeats);
            } else if !status.online {
                status.missed_heartbeats = expected_heartbeats_missed as u32;
            }
        }
    }
}

lazy_static! {
    pub static ref SESSIONS: std::sync::RwLock<std::collections::HashMap<String, Session>> =
        std::sync::RwLock::new(std::collections::HashMap::new());
}

pub fn create_session(device_id: String, user_address: String) -> Session {
    let devices = get_mock_devices();
    let device_name = devices
        .iter()
        .find(|d| d.id == device_id)
        .map(|d| d.name.clone())
        .unwrap_or_else(|| "Unknown Device".to_string());

    let session = Session::new(device_id, device_name, user_address);
    let mut sessions = SESSIONS.write().unwrap();
    sessions.insert(session.id.clone(), session.clone());
    session
}

pub fn get_session(id: &str) -> Option<Session> {
    let sessions = SESSIONS.read().unwrap();
    sessions.get(id).cloned()
}

pub fn get_sessions_by_user(user_address: &str) -> Vec<Session> {
    let sessions = SESSIONS.read().unwrap();
    sessions
        .values()
        .filter(|s| s.user_address == user_address)
        .cloned()
        .collect()
}

pub fn extend_session(id: &str, hours: i64) -> Result<Session, String> {
    let mut sessions = SESSIONS.write().unwrap();
    if let Some(session) = sessions.get_mut(id) {
        if !session.active || session.expires_at < chrono::Utc::now() {
            return Err("Session has expired".to_string());
        }
        session.expires_at = session.expires_at + chrono::Duration::hours(hours);
        Ok(session.clone())
    } else {
        Err("Session not found".to_string())
    }
}

pub fn end_session(id: &str) -> Result<(), String> {
    let mut sessions = SESSIONS.write().unwrap();
    if let Some(session) = sessions.get_mut(id) {
        session.active = false;
        Ok(())
    } else {
        Err("Session not found".to_string())
    }
}

lazy_static! {
    pub static ref TELEMETRY_STORE: std::sync::RwLock<std::collections::HashMap<String, Vec<TelemetryData>>> =
        std::sync::RwLock::new(std::collections::HashMap::new());

    pub static ref TELEMETRY_CHANNELS: std::sync::RwLock<std::collections::HashMap<String, broadcast::Sender<TelemetryData>>> =
        std::sync::RwLock::new(std::collections::HashMap::new());
}

pub fn ingest_telemetry(device_id: &str, data: Vec<TelemetryData>) {
    let mut store = TELEMETRY_STORE.write().unwrap();
    let device_store = store.entry(device_id.to_string()).or_insert_with(Vec::new);
    device_store.extend(data.clone());

    let channels = TELEMETRY_CHANNELS.read().unwrap();
    if let Some(tx) = channels.get(device_id) {
        for item in data {
            let _ = tx.send(item);
        }
    }
}

pub fn subscribe_telemetry(device_id: &str) -> broadcast::Receiver<TelemetryData> {
    let mut channels = TELEMETRY_CHANNELS.write().unwrap();
    let tx = channels.entry(device_id.to_string()).or_insert_with(|| {
        let (tx, _) = broadcast::channel(100);
        tx
    });
    tx.subscribe()
}

pub fn generate_telemetry_data(device_category: &DeviceCategory, ticks: u64) -> TelemetryData {
    use std::collections::HashMap;
    let now = chrono::Utc::now().to_rfc3339();
    let mut numeric_readings = HashMap::new();
    let mut boolean_readings = HashMap::new();
    let mut string_readings = HashMap::new();
    let mut is_abnormal = false;

    match device_category {
        DeviceCategory::Climate | DeviceCategory::Environmental => {
            // Temperature, Humidity
            let base_temp = 22.0;
            // Introduce a periodic abnormal reading every 15 ticks for demonstration
            let temp = if ticks % 15 == 0 {
                is_abnormal = true;
                38.5 // Abnormal high temp
            } else {
                base_temp + (ticks as f64 * 0.1).sin() * 2.0 + rand_noise(ticks)
            };
            
            let humidity = 45.0 + (ticks as f64 * 0.05).cos() * 5.0 + rand_noise(ticks);

            numeric_readings.insert("temperature".to_string(), (temp * 10.0).round() / 10.0);
            numeric_readings.insert("humidity".to_string(), (humidity * 10.0).round() / 10.0);

            let hvac = temp > 23.0;
            boolean_readings.insert("hvac_active".to_string(), hvac);
            boolean_readings.insert("filter_clogged".to_string(), false);

            string_readings.insert(
                "status".to_string(),
                if is_abnormal {
                    "Critical: High Temperature!".to_string()
                } else if hvac {
                    "HVAC Cooling Active".to_string()
                } else {
                    "System Optimal".to_string()
                }
            );
        }
        DeviceCategory::Security | DeviceCategory::Access => {
            // Motion Detector / Lock
            // Let's trigger a security alarm every 20 ticks
            let alarm = ticks % 20 == 0;
            if alarm {
                is_abnormal = true;
            }

            numeric_readings.insert("motion_count".to_string(), (ticks / 10) as f64);
            numeric_readings.insert("battery_level".to_string(), (100.0 - (ticks as f64 * 0.01)).clamp(0.0, 100.0));

            boolean_readings.insert("motion_detected".to_string(), alarm);
            boolean_readings.insert("tamper_sensor".to_string(), false);
            boolean_readings.insert("door_locked".to_string(), !alarm);

            string_readings.insert(
                "status".to_string(),
                if alarm {
                    "ALERT: Intrusion Detected!".to_string()
                } else {
                    "Secure".to_string()
                }
            );
        }
        _ => {
            // Utility / other (e.g. Water Flow Sensor)
            let leak = ticks % 18 == 0;
            if leak {
                is_abnormal = true;
            }

            let flow_rate = if leak {
                15.4
            } else if ticks % 5 == 0 {
                0.0
            } else {
                3.2 + (ticks as f64 * 0.2).sin() * 0.5 + rand_noise(ticks)
            };

            numeric_readings.insert("flow_rate".to_string(), (flow_rate * 10.0).round() / 10.0);
            numeric_readings.insert("total_flow".to_string(), (ticks as f64 * 0.5));

            boolean_readings.insert("valve_open".to_string(), flow_rate > 0.0);
            boolean_readings.insert("leak_detected".to_string(), leak);

            string_readings.insert(
                "status".to_string(),
                if leak {
                    "CRITICAL: Leak Detected!".to_string()
                } else if flow_rate > 0.0 {
                    "Flowing".to_string()
                } else {
                    "Idle (No Flow)".to_string()
                }
            );
        }
    }

            TelemetryData {
        timestamp: now,
        numeric_readings,
        boolean_readings,
        string_readings,
        is_abnormal,
    }
}

pub fn enrich_devices_with_ratings(devices: &mut Vec<Device>) {
    let reviews = REVIEWS.read().unwrap();
    for d in devices {
        if let Some(device_reviews) = reviews.get(&d.id) {
            if !device_reviews.is_empty() {
                let sum: u64 = device_reviews.iter().map(|r| r.rating as u64).sum();
                d.rating = sum as f64 / device_reviews.len() as f64;
            }
        }
    }
}

lazy_static! {
    pub static ref REVIEWS: std::sync::RwLock<std::collections::HashMap<String, Vec<Review>>> =
        std::sync::RwLock::new(std::collections::HashMap::new());
}

pub fn add_review(device_id: &str, req: ReviewRequest) -> Result<Review, String> {
    if req.rating < 1 || req.rating > 5 {
        return Err("Rating must be between 1 and 5".to_string());
    }
    if req.comment.len() > 1000 {
        return Err("Comment is too long".to_string());
    }

    let sessions = SESSIONS.read().unwrap();
    let has_session = sessions.values().any(|s| s.user_address == req.user_address && s.device_id == device_id);
    if !has_session {
        return Err("Must have an active or past session to review (verified purchase only)".to_string());
    }

    let review = Review {
        id: uuid::Uuid::new_v4().to_string(),
        device_id: device_id.to_string(),
        user_address: req.user_address,
        rating: req.rating,
        comment: req.comment,
        verified_purchase: true,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let mut reviews = REVIEWS.write().unwrap();
    reviews.entry(device_id.to_string()).or_insert_with(Vec::new).push(review.clone());

    Ok(review)
}

pub fn get_reviews(device_id: &str) -> Vec<Review> {
    let reviews = REVIEWS.read().unwrap();
    reviews.get(device_id).cloned().unwrap_or_default()
}

/// Process a withdrawal request — mock implementation.
///
/// In production this would create and submit a Stellar payment transaction
/// using the Stellar SDK, and the frontend would sign it via Freighter.
pub fn process_withdrawal(
    owner_address: &str,
    amount: f64,
    destination: &str,
) -> Result<(String, f64), String> {
    if amount <= 0.0 {
        return Err("Amount must be positive".to_string());
    }
    if owner_address.is_empty() || destination.is_empty() {
        return Err("Owner and destination addresses are required".to_string());
    }
    // Simulate a Stellar transaction hash.
    let tx_hash = format!(
        "a1b2c3d4e5f6{}{}",
        &crate::analytics::fnv1a_hash(owner_address).to_string()[..8],
        &crate::analytics::fnv1a_hash(destination).to_string()[..8],
    );
    // Simulate a nominal network fee in stroops (1 stroop = 0.0000001 XLM).
    let fee = 0.00001;
    Ok((tx_hash, fee))
}

fn rand_noise(ticks: u64) -> f64 {
    // Deterministic pseudo-random noise based on ticks
    ((ticks * 12345) % 100) as f64 / 500.0 - 0.1
}

// ─── Payment History ────────────────────────────────────────────────────────────

lazy_static! {
    pub static ref PAYMENTS: std::sync::RwLock<Vec<PaymentRecord>> =
        std::sync::RwLock::new(Vec::new());
}

/// Persist a verified payment so it appears in the user's transaction history.
pub fn record_payment(tx_hash: &str, amount: f64, session: &Session) {
    let record = PaymentRecord {
        id: uuid::Uuid::new_v4().to_string(),
        tx_hash: tx_hash.to_string(),
        device_id: session.device_id.clone(),
        device_name: session.device_name.clone(),
        user_address: session.user_address.clone(),
        amount,
        session_id: session.id.clone(),
        created_at: session.created_at,
        expires_at: session.expires_at,
    };
    PAYMENTS.write().unwrap().push(record);
}

/// Derive the live status (`active` | `expired` | `ended`) and the session
/// duration in seconds for a payment record.
fn payment_status_and_duration(rec: &PaymentRecord) -> (String, i64) {
    let now = Utc::now();
    match get_session(&rec.session_id) {
        Some(s) if s.active && s.expires_at > now => {
            ("active".to_string(), (now - rec.created_at).num_seconds().max(0))
        }
        Some(s) if !s.active => {
            let end = now.min(s.expires_at);
            ("ended".to_string(), (end - rec.created_at).num_seconds().max(0))
        }
        _ => (
            "expired".to_string(),
            (rec.expires_at - rec.created_at).num_seconds().max(0),
        ),
    }
}

/// Parse a filter bound that may be a full RFC-3339 timestamp or a `YYYY-MM-DD`
/// date. `end_of_day` extends a bare date to 23:59:59 so `to=` is inclusive.
fn parse_date_bound(value: &str, end_of_day: bool) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let time = if end_of_day {
            chrono::NaiveTime::from_hms_opt(23, 59, 59)?
        } else {
            chrono::NaiveTime::from_hms_opt(0, 0, 0)?
        };
        return Some(DateTime::from_naive_utc_and_offset(date.and_time(time), Utc));
    }
    None
}

/// Build a filtered, aggregated payment history for a user.
pub fn get_payment_history(query: &PaymentHistoryQuery) -> PaymentHistoryResponse {
    let from = query.from.as_deref().and_then(|v| parse_date_bound(v, false));
    let to = query.to.as_deref().and_then(|v| parse_date_bound(v, true));
    let status_filter = query.status.as_deref().map(|s| s.to_lowercase());

    let payments = PAYMENTS.read().unwrap();
    let mut entries: Vec<PaymentHistoryEntry> = payments
        .iter()
        .filter(|p| p.user_address == query.user)
        .filter(|p| {
            query
                .device_id
                .as_ref()
                .map(|id| &p.device_id == id)
                .unwrap_or(true)
        })
        .filter(|p| from.map(|f| p.created_at >= f).unwrap_or(true))
        .filter(|p| to.map(|t| p.created_at <= t).unwrap_or(true))
        .map(|p| {
            let (status, duration_secs) = payment_status_and_duration(p);
            PaymentHistoryEntry {
                id: p.id.clone(),
                tx_hash: p.tx_hash.clone(),
                device_id: p.device_id.clone(),
                device_name: p.device_name.clone(),
                user_address: p.user_address.clone(),
                amount: p.amount,
                session_id: p.session_id.clone(),
                created_at: p.created_at.to_rfc3339(),
                expires_at: p.expires_at.to_rfc3339(),
                status,
                duration_secs,
            }
        })
        .filter(|e| {
            status_filter
                .as_ref()
                .map(|s| &e.status == s)
                .unwrap_or(true)
        })
        .collect();

    // Newest first.
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let total_spent = round2(entries.iter().map(|e| e.amount).sum());
    let total_duration_secs = entries.iter().map(|e| e.duration_secs).sum();

    PaymentHistoryResponse {
        total_sessions: entries.len(),
        total_spent,
        total_duration_secs,
        data: entries,
    }
}

/// Render a payment history as RFC-4180 CSV for export.
pub fn payment_history_to_csv(report: &PaymentHistoryResponse) -> Result<String, String> {
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_writer(vec![]);

    wtr.write_record([
        "date",
        "device_id",
        "device_name",
        "amount_xlm",
        "status",
        "duration_secs",
        "session_id",
        "tx_hash",
    ])
    .map_err(|e| e.to_string())?;

    for e in &report.data {
        wtr.write_record([
            &e.created_at,
            &e.device_id,
            &e.device_name,
            &e.amount.to_string(),
            &e.status,
            &e.duration_secs.to_string(),
            &e.session_id,
            &e.tx_hash,
        ])
        .map_err(|e| e.to_string())?;
    }

    // Totals footer.
    wtr.write_record([""]).map_err(|e| e.to_string())?;
    wtr.write_record(["# Totals"]).map_err(|e| e.to_string())?;
    wtr.write_record(["total_spent_xlm", &report.total_spent.to_string()])
        .map_err(|e| e.to_string())?;
    wtr.write_record(["total_sessions", &report.total_sessions.to_string()])
        .map_err(|e| e.to_string())?;
    wtr.write_record(["total_duration_secs", &report.total_duration_secs.to_string()])
        .map_err(|e| e.to_string())?;

    let bytes = wtr.into_inner().map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn round2(v: f64) -> f64 {
    // `+ 0.0` normalises any negative zero to positive zero.
    (v * 100.0).round() / 100.0 + 0.0
}

// ─── QR Code Scan Analytics ─────────────────────────────────────────────────────

#[derive(Clone)]
struct QrScan {
    timestamp: DateTime<Utc>,
    #[allow(dead_code)]
    source: Option<String>,
}

lazy_static! {
    static ref QR_SCANS: std::sync::RwLock<std::collections::HashMap<String, Vec<QrScan>>> =
        std::sync::RwLock::new(std::collections::HashMap::new());
}

/// Record a QR-code scan for a device. Returns `Err` if the device is unknown.
pub fn record_qr_scan(device_id: &str, source: Option<String>) -> Result<(), String> {
    if crate::device_registry::get(device_id).is_none() {
        return Err("Device not found".to_string());
    }
    let mut scans = QR_SCANS.write().unwrap();
    scans
        .entry(device_id.to_string())
        .or_insert_with(Vec::new)
        .push(QrScan {
            timestamp: Utc::now(),
            source,
        });
    Ok(())
}

/// Aggregate QR-scan analytics for a device (total, last scan, per-day counts).
pub fn get_qr_analytics(device_id: &str) -> QrScanAnalytics {
    let scans = QR_SCANS.read().unwrap();
    let device_scans = scans.get(device_id).cloned().unwrap_or_default();

    let total_scans = device_scans.len() as u64;
    let last_scan = device_scans
        .iter()
        .map(|s| s.timestamp)
        .max()
        .map(|t| t.to_rfc3339());

    // Group by calendar day (UTC).
    let mut by_day: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for scan in &device_scans {
        let day = scan.timestamp.date_naive().to_string();
        *by_day.entry(day).or_insert(0) += 1;
    }
    let daily = by_day
        .into_iter()
        .map(|(date, scans)| QrScanDailyPoint { date, scans })
        .collect();

    QrScanAnalytics {
        device_id: device_id.to_string(),
        total_scans,
        last_scan,
        daily,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DeviceCategory, DeviceSearchQuery, SortField, SortOrder};

    fn base_query() -> DeviceSearchQuery {
        DeviceSearchQuery::default()
    }

    #[test]
    fn test_no_filters_returns_all_devices() {
        let resp = search_devices(&base_query());
        assert_eq!(resp.total, 10);
        assert_eq!(resp.data.len(), 10);
        assert!(resp.next_cursor.is_none());
    }

    #[test]
    fn test_full_text_search_by_name() {
        let q = DeviceSearchQuery {
            q: Some("lock".to_string()),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert_eq!(resp.total, 1);
        assert_eq!(resp.data[0].id, "device-001");
    }

    #[test]
    fn test_full_text_search_by_description() {
        let q = DeviceSearchQuery {
            q: Some("night vision".to_string()),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert_eq!(resp.total, 1);
        assert_eq!(resp.data[0].id, "device-003");
    }

    #[test]
    fn test_full_text_search_case_insensitive() {
        let q = DeviceSearchQuery {
            q: Some("SENSOR".to_string()),
            ..base_query()
        };
        let resp = search_devices(&q);
        // Temperature Sensor, Water Flow Sensor, Humidity Sensor, CO2 sensor (description)
        assert!(resp.total >= 3);
    }

    #[test]
    fn test_full_text_no_match_returns_empty() {
        let q = DeviceSearchQuery {
            q: Some("xyznotadevice".to_string()),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert_eq!(resp.total, 0);
        assert!(resp.data.is_empty());
    }

    #[test]
    fn test_filter_by_category_security() {
        let q = DeviceSearchQuery {
            category: Some(DeviceCategory::Security),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert_eq!(resp.total, 2); // Security Camera + Motion Detector Pro
        assert!(resp
            .data
            .iter()
            .all(|d| d.category == DeviceCategory::Security));
    }

    #[test]
    fn test_filter_by_category_environmental() {
        let q = DeviceSearchQuery {
            category: Some(DeviceCategory::Environmental),
            ..base_query()
        };
        let resp = search_devices(&q);
        // Temperature Sensor, Air Quality Monitor, Humidity Sensor, CO2 Monitor
        assert_eq!(resp.total, 4);
    }

    #[test]
    fn test_filter_available_only() {
        let q = DeviceSearchQuery {
            available: Some(true),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert!(resp.data.iter().all(|d| d.available));
        assert_eq!(resp.total, 8); // 10 total − 2 unavailable
    }

    #[test]
    fn test_filter_unavailable_only() {
        let q = DeviceSearchQuery {
            available: Some(false),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert!(resp.data.iter().all(|d| !d.available));
        assert_eq!(resp.total, 2);
    }

    #[test]
    fn test_filter_min_price() {
        let q = DeviceSearchQuery {
            min_price: Some(7.0),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert!(resp.data.iter().all(|d| d.price >= 7.0));
    }

    #[test]
    fn test_filter_max_price() {
        let q = DeviceSearchQuery {
            max_price: Some(3.0),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert!(resp.data.iter().all(|d| d.price <= 3.0));
    }

    #[test]
    fn test_filter_price_range() {
        let q = DeviceSearchQuery {
            min_price: Some(3.0),
            max_price: Some(6.0),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert!(resp.data.iter().all(|d| d.price >= 3.0 && d.price <= 6.0));
    }

    #[test]
    fn test_sort_by_price_asc() {
        let q = DeviceSearchQuery {
            sort_by: Some(SortField::Price),
            sort_order: SortOrder::Asc,
            ..base_query()
        };
        let resp = search_devices(&q);
        let prices: Vec<f64> = resp.data.iter().map(|d| d.price).collect();
        let mut sorted = prices.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(prices, sorted);
    }

    #[test]
    fn test_sort_by_price_desc() {
        let q = DeviceSearchQuery {
            sort_by: Some(SortField::Price),
            sort_order: SortOrder::Desc,
            ..base_query()
        };
        let resp = search_devices(&q);
        let prices: Vec<f64> = resp.data.iter().map(|d| d.price).collect();
        let mut sorted = prices.clone();
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap());
        assert_eq!(prices, sorted);
    }

    #[test]
    fn test_sort_by_rating_desc() {
        let q = DeviceSearchQuery {
            sort_by: Some(SortField::Rating),
            sort_order: SortOrder::Desc,
            ..base_query()
        };
        let resp = search_devices(&q);
        let ratings: Vec<f64> = resp.data.iter().map(|d| d.rating).collect();
        for w in ratings.windows(2) {
            assert!(w[0] >= w[1]);
        }
    }

    #[test]
    fn test_sort_by_popularity_desc() {
        let q = DeviceSearchQuery {
            sort_by: Some(SortField::Popularity),
            sort_order: SortOrder::Desc,
            ..base_query()
        };
        let resp = search_devices(&q);
        let pop: Vec<u64> = resp.data.iter().map(|d| d.popularity).collect();
        for w in pop.windows(2) {
            assert!(w[0] >= w[1]);
        }
    }

    #[test]
    fn test_geospatial_filter_tight_radius() {
        // Centre on device-001 (37.7749, -122.4194) with 0.5 km radius.
        let q = DeviceSearchQuery {
            lat: Some(37.7749),
            lng: Some(-122.4194),
            radius_km: Some(0.5),
            ..base_query()
        };
        let resp = search_devices(&q);
        // All mock devices are within ~0.5 km of each other, so at least
        // device-001 must be returned; many others likely too.
        assert!(resp.total >= 1);
        assert!(resp.data.iter().any(|d| d.id == "device-001"));
    }

    #[test]
    fn test_geospatial_filter_zero_radius_returns_none() {
        // A point far from all devices returns no results.
        let q = DeviceSearchQuery {
            lat: Some(0.0),
            lng: Some(0.0),
            radius_km: Some(0.01),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert_eq!(resp.total, 0);
    }

    #[test]
    fn test_pagination_limit() {
        let q = DeviceSearchQuery {
            limit: Some(3),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert_eq!(resp.data.len(), 3);
        assert_eq!(resp.limit, 3);
        assert!(resp.next_cursor.is_some());
    }

    #[test]
    fn test_pagination_cursor_next_page() {
        let q1 = DeviceSearchQuery {
            limit: Some(3),
            ..base_query()
        };
        let resp1 = search_devices(&q1);
        assert_eq!(resp1.data.len(), 3);

        let cursor = resp1.next_cursor.clone().unwrap();
        let q2 = DeviceSearchQuery {
            limit: Some(3),
            cursor: Some(cursor),
            ..base_query()
        };
        let resp2 = search_devices(&q2);
        assert_eq!(resp2.data.len(), 3);

        // Pages must not overlap
        let ids1: Vec<&str> = resp1.data.iter().map(|d| d.id.as_str()).collect();
        let ids2: Vec<&str> = resp2.data.iter().map(|d| d.id.as_str()).collect();
        for id in &ids2 {
            assert!(!ids1.contains(id));
        }
    }

    #[test]
    fn test_pagination_last_page_has_no_cursor() {
        // Fetch all results in one page
        let q = DeviceSearchQuery {
            limit: Some(100),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert_eq!(resp.total, 10);
        assert_eq!(resp.data.len(), 10);
        assert!(resp.next_cursor.is_none());
    }

    #[test]
    fn test_combined_search_filter_sort_pagination() {
        let q = DeviceSearchQuery {
            q: Some("sensor".to_string()),
            available: Some(true),
            sort_by: Some(SortField::Price),
            sort_order: SortOrder::Asc,
            limit: Some(2),
            ..base_query()
        };
        let resp = search_devices(&q);
        // All results must match "sensor" in name/description, be available, sorted by price
        for d in &resp.data {
            let text = format!("{} {}", d.name.to_lowercase(), d.description.to_lowercase());
            assert!(text.contains("sensor"));
            assert!(d.available);
        }
        if resp.data.len() > 1 {
            assert!(resp.data[0].price <= resp.data[1].price);
        }
    }

    #[test]
    fn test_limit_clamped_to_100() {
        let q = DeviceSearchQuery {
            limit: Some(999),
            ..base_query()
        };
        let resp = search_devices(&q);
        assert!(resp.limit <= 100);
    }

    #[test]
    fn test_haversine_distance() {
        // San Francisco → Los Angeles ≈ 559 km
        let dist = haversine_km(37.7749, -122.4194, 34.0522, -118.2437);
        assert!(
            (dist - 559.0).abs() < 10.0,
            "expected ~559 km, got {}",
            dist
        );
    }

    #[test]
    fn test_verify_payment_device_not_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(verify_payment(
            "tx_hash",
            "non-existent-device",
            "GABCDEF123",
        ));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Device not found");
    }

    // ── Payment history ──────────────────────────────────────────────────────

    fn history_query(user: &str) -> PaymentHistoryQuery {
        PaymentHistoryQuery {
            user: user.to_string(),
            device_id: None,
            status: None,
            from: None,
            to: None,
            format: None,
        }
    }

    #[test]
    fn test_payment_history_records_and_aggregates() {
        let user = "GUSERPAYMENTHISTAGG";
        let session =
            Session::new("device-001".into(), "Smart Lock Alpha".into(), user.into());
        SESSIONS
            .write()
            .unwrap()
            .insert(session.id.clone(), session.clone());
        record_payment("txhashAGG", 5.0, &session);

        let resp = get_payment_history(&history_query(user));
        assert_eq!(resp.total_sessions, 1);
        assert_eq!(resp.total_spent, 5.0);
        assert_eq!(resp.data[0].status, "active");
        assert_eq!(resp.data[0].tx_hash, "txhashAGG");
        assert!(resp.data[0].duration_secs >= 0);
    }

    #[test]
    fn test_payment_history_csv_export() {
        let user = "GUSERPAYMENTHISTCSV";
        let session = Session::new("device-002".into(), "Temperature Sensor".into(), user.into());
        SESSIONS
            .write()
            .unwrap()
            .insert(session.id.clone(), session.clone());
        record_payment("txhashCSV", 2.5, &session);

        let resp = get_payment_history(&history_query(user));
        let csv = payment_history_to_csv(&resp).expect("csv should render");
        assert!(csv.contains("txhashCSV"));
        assert!(csv.contains("# Totals"));
        assert!(csv.contains("total_spent_xlm"));
    }

    #[test]
    fn test_payment_history_empty_user_is_zeroed() {
        let resp = get_payment_history(&history_query("GNOTAREALUSERXYZ"));
        assert_eq!(resp.total_sessions, 0);
        assert_eq!(resp.total_spent, 0.0);
        // Negative zero must be normalised.
        assert!(resp.total_spent.is_sign_positive());
    }

    #[test]
    fn test_payment_history_filters_by_device() {
        let user = "GUSERPAYMENTHISTDEV";
        for (dev, name) in [("device-001", "Smart Lock Alpha"), ("device-002", "Temperature Sensor")] {
            let s = Session::new(dev.into(), name.into(), user.into());
            SESSIONS.write().unwrap().insert(s.id.clone(), s.clone());
            record_payment(&format!("tx-{dev}"), 1.0, &s);
        }
        let mut q = history_query(user);
        q.device_id = Some("device-002".into());
        let resp = get_payment_history(&q);
        assert_eq!(resp.total_sessions, 1);
        assert_eq!(resp.data[0].device_id, "device-002");
    }

    // ── QR scan analytics ────────────────────────────────────────────────────

    #[test]
    fn test_qr_analytics_counts_scans() {
        let req = crate::models::DeviceRegistrationRequest {
            name: "QR Test Device".into(),
            device_type: crate::models::DeviceType::Sensor,
            description: "qr".into(),
            price: 1.0,
            location: "Lab".into(),
            connectivity: "wifi".into(),
            owner_address: format!("G{}", "C".repeat(55)),
            latitude: None,
            longitude: None,
        };
        let dev = crate::device_registry::register(req).unwrap();

        record_qr_scan(&dev.id, Some("print".into())).unwrap();
        record_qr_scan(&dev.id, None).unwrap();

        let analytics = get_qr_analytics(&dev.id);
        assert_eq!(analytics.total_scans, 2);
        assert!(analytics.last_scan.is_some());
        assert_eq!(analytics.daily.iter().map(|d| d.scans).sum::<u64>(), 2);

        crate::device_registry::delete(&dev.id, Some(&dev.owner_address)).unwrap();
    }

    #[test]
    fn test_qr_scan_unknown_device_errors() {
        assert!(record_qr_scan("device-does-not-exist", None).is_err());
    }
}
