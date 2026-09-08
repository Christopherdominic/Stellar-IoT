/// Analytics service — generates device usage and revenue reports.
///
/// The project currently uses in-memory mock data rather than a persistent
/// database, so this module synthesises deterministic but realistic analytics
/// from the same device catalogue used by the rest of the API.  Every value is
/// derived from the device's `popularity` and `price` fields so the numbers are
/// internally consistent across endpoints and respond correctly to the chosen
/// `period` / `lookback` query parameters.
use crate::models::{
    AnalyticsQuery, DeviceAnalyticsReport, OwnerEarningsQuery, OwnerEarningsResponse,
    OwnerDeviceStatus, PeakHour, ReportPeriod, RetentionRow, TimeSeriesPoint, TopDevice,
};
use crate::services::{get_mock_devices, DEVICE_STATUSES};
use chrono::{Datelike, Duration, Utc};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Step size in days for each period granularity.
fn period_days(period: &ReportPeriod) -> i64 {
    match period {
        ReportPeriod::Daily => 1,
        ReportPeriod::Weekly => 7,
        ReportPeriod::Monthly => 30,
    }
}

/// Default look-back count per granularity.
fn default_lookback(period: &ReportPeriod) -> usize {
    match period {
        ReportPeriod::Daily => 30,
        ReportPeriod::Weekly => 12,
        ReportPeriod::Monthly => 12,
    }
}

/// Simple deterministic pseudo-random based on a seed integer.
/// Returns a float in [0, 1).  We avoid external rand crate to keep deps minimal.
fn pseudo_rand(seed: u64) -> f64 {
    // Xorshift64
    let mut x = seed ^ (seed << 13);
    x ^= x >> 7;
    x ^= x << 17;
    (x % 10_000) as f64 / 10_000.0
}

// ─── Owner earnings analytics ────────────────────────────────────────────────

/// Generate an aggregate earnings report for all devices owned by `owner_address`.
pub fn generate_owner_report(query: &OwnerEarningsQuery) -> OwnerEarningsResponse {
    let period = &query.period;
    let lookback = query.lookback.unwrap_or_else(|| default_lookback(period));
    let step = period_days(period);
    let today = Utc::now().date_naive();

    let devices = get_mock_devices()
        .into_iter()
        .filter(|d| d.owner_address == query.owner_address)
        .collect::<Vec<_>>();

    let total_devices = devices.len();

    // Aggregate time-series across all owned devices.
    let mut time_series_map: std::collections::HashMap<String, (f64, u64)> =
        std::collections::HashMap::new();

    let mut overall_revenue = 0.0f64;
    let mut overall_sessions = 0u64;
    let mut top_devices: Vec<TopDevice> = Vec::with_capacity(total_devices);

    for device in &devices {
        let base_sessions_per_period =
            (device.popularity as f64 / 365.0 * step as f64).max(1.0) as u64;
        let mut device_revenue = 0.0f64;
        let mut device_sessions = 0u64;

        for i in (0..lookback).rev() {
            let period_start = today - Duration::days(step * (i as i64 + 1) - step);
            let period_start = period_start
                - Duration::days(match period {
                    ReportPeriod::Weekly => period_start.weekday().num_days_from_monday() as i64,
                    ReportPeriod::Monthly => (period_start.day0()) as i64,
                    ReportPeriod::Daily => 0,
                });

            let seed = fnv1a_hash(&device.id) ^ (i as u64 * 6_364_136_223_846_793_005);
            let variance = 0.6 + pseudo_rand(seed) * 0.8;
            let sessions = ((base_sessions_per_period as f64) * variance).round() as u64;
            let revenue = sessions as f64 * device.price;

            let entry = time_series_map
                .entry(period_start.to_string())
                .or_insert((0.0, 0));
            entry.0 += revenue;
            entry.1 += sessions;

            device_revenue += revenue;
            device_sessions += sessions;
        }

        overall_revenue += device_revenue;
        overall_sessions += device_sessions;

        // Simulate uptime from health status.
        let uptime_pct = get_device_uptime(&device.id).unwrap_or(99.0);

        top_devices.push(TopDevice {
            id: device.id.clone(),
            name: device.name.clone(),
            earnings: round2(device_revenue),
            sessions: device_sessions,
            uptime_pct,
        });
    }

    // Sort top devices by earnings descending.
    top_devices.sort_by(|a, b| b.earnings.partial_cmp(&a.earnings).unwrap());

    // Build sorted time-series.
    let mut time_series: Vec<TimeSeriesPoint> = time_series_map
        .into_iter()
        .map(|(date, (revenue, session_count))| TimeSeriesPoint {
            date,
            revenue: round2(revenue),
            session_count,
            unique_users: 0,
        })
        .collect();
    time_series.sort_by(|a, b| a.date.cmp(&b.date));

    let uptime_avg = if total_devices > 0 {
        let sum: f64 = devices.iter().filter_map(|d| get_device_uptime(&d.id)).sum();
        round2(sum / total_devices as f64)
    } else {
        100.0
    };

    // Pending ≈ 10 % of total (simulating unconfirmed payments in pipeline).
    let pending = round2(overall_revenue * 0.10);

    let period_label = match period {
        ReportPeriod::Daily => "daily",
        ReportPeriod::Weekly => "weekly",
        ReportPeriod::Monthly => "monthly",
    };

    OwnerEarningsResponse {
        total_earnings_xlm: round2(overall_revenue),
        pending_earnings_xlm: pending,
        total_devices,
        total_sessions: overall_sessions,
        period: period_label.to_string(),
        time_series,
        top_devices,
        uptime_avg,
    }
}

/// Return per-device status and earnings for the owner's device list view.
pub fn get_owner_device_statuses(owner_address: &str) -> Vec<OwnerDeviceStatus> {
    let devices = get_mock_devices()
        .into_iter()
        .filter(|d| d.owner_address == owner_address)
        .collect::<Vec<_>>();

    let today = Utc::now().date_naive();

    devices
        .into_iter()
        .map(|d| {
            let step = 1i64;
            let base_sessions =
                (d.popularity as f64 / 365.0 * step as f64).max(1.0) as u64;
            let mut total_sessions = 0u64;

            for i in (0..30).rev() {
                let seed = fnv1a_hash(&d.id) ^ (i as u64 * 6_364_136_223_846_793_005);
                let variance = 0.6 + pseudo_rand(seed) * 0.8;
                let sessions = ((base_sessions as f64) * variance).round() as u64;
                total_sessions += sessions;
            }

            let total_earnings = total_sessions as f64 * d.price;
            let uptime_pct = get_device_uptime(&d.id).unwrap_or(99.0);
            let last_seen = DEVICE_STATUSES
                .read()
                .unwrap()
                .get(&d.id)
                .and_then(|s| s.last_seen.map(|t| t.to_rfc3339()));
            let online = DEVICE_STATUSES
                .read()
                .unwrap()
                .get(&d.id)
                .map(|s| s.online)
                .unwrap_or(true);

            OwnerDeviceStatus {
                id: d.id,
                name: d.name,
                online,
                uptime_pct,
                last_seen,
                total_sessions,
                total_earnings: round2(total_earnings),
                price: d.price,
            }
        })
        .collect()
}

/// Deterministic uptime percentage derived from device id hash.
fn get_device_uptime(device_id: &str) -> Option<f64> {
    // Check actual status first; fall back to deterministic mock value.
    let statuses = DEVICE_STATUSES.read().ok()?;
    if let Some(status) = statuses.get(device_id) {
        if !status.online {
            return Some(85.0 + pseudo_rand(fnv1a_hash(device_id)) * 10.0);
        }
    }
    Some(95.0 + pseudo_rand(fnv1a_hash(device_id) ^ 0xABCD) * 4.9)
}

// ─── Core analytics generator ────────────────────────────────────────────────

pub fn generate_report(device_id: &str, query: &AnalyticsQuery) -> Option<DeviceAnalyticsReport> {
    let device = get_mock_devices()
        .into_iter()
        .find(|d| d.id == device_id)?;

    let period = &query.period;
    let lookback = query.lookback.unwrap_or_else(|| default_lookback(period));
    let step = period_days(period);

    let today = Utc::now().date_naive();

    // ── Build time-series ────────────────────────────────────────────────────
    // Popularity is total lifetime accesses; distribute across the lookback
    // window with a realistic daily variance.
    let base_sessions_per_period =
        (device.popularity as f64 / 365.0 * step as f64).max(1.0) as u64;

    let mut time_series: Vec<TimeSeriesPoint> = Vec::with_capacity(lookback);
    let mut all_users: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut total_revenue = 0.0f64;
    let mut total_sessions = 0u64;

    for i in (0..lookback).rev() {
        let period_start = today - Duration::days(step * (i as i64 + 1) - step);
        let period_start = period_start
            - Duration::days(
                // Snap weekly/monthly to period boundaries for cleaner labels.
                match period {
                    ReportPeriod::Weekly => period_start.weekday().num_days_from_monday() as i64,
                    ReportPeriod::Monthly => (period_start.day0()) as i64,
                    ReportPeriod::Daily => 0,
                },
            );

        // Deterministic variance: combine device id hash + period index.
        let seed = fnv1a_hash(device_id) ^ (i as u64 * 6_364_136_223_846_793_005);
        let variance = 0.6 + pseudo_rand(seed) * 0.8; // ±40 % swing

        let sessions = ((base_sessions_per_period as f64) * variance).round() as u64;
        let revenue = sessions as f64 * device.price;

        // Simulate unique users as ~70–95 % of sessions (repeat visitors).
        let unique_seed = seed ^ 0xDEAD_BEEF;
        let unique_ratio = 0.70 + pseudo_rand(unique_seed) * 0.25;
        let unique = ((sessions as f64) * unique_ratio).ceil() as u64;

        // Populate a fake user-address pool for retention calculation.
        for u in 0..unique {
            let addr = format!("G{:055}", fnv1a_hash(&format!("{}{}{}", device_id, i, u)));
            all_users.insert(addr);
        }

        total_revenue += revenue;
        total_sessions += sessions;

        time_series.push(TimeSeriesPoint {
            date: period_start.to_string(),
            revenue: round2(revenue),
            session_count: sessions,
            unique_users: unique,
        });
    }

    let total_unique_users = all_users.len() as u64;

    // ── Average session duration ─────────────────────────────────────────────
    // Devices with higher ratings keep users engaged longer (scaled from 5–60 min).
    let avg_session_duration_secs = 300.0 + (device.rating / 5.0) * 3_300.0;

    // ── Peak hours ───────────────────────────────────────────────────────────
    // Hardcode a realistic bimodal distribution (morning rush + afternoon peak)
    // biased by the device's location index.
    let device_bias = (fnv1a_hash(device_id) % 3) as u8; // 0,1,2
    let raw_peaks: [(u8, f64); 24] = [
        (0, 0.3),
        (1, 0.2),
        (2, 0.1),
        (3, 0.1),
        (4, 0.1),
        (5, 0.2),
        (6, 0.5 + device_bias as f64 * 0.1),
        (7, 1.0 + device_bias as f64 * 0.2),
        (8, 1.5),
        (9, 1.8),
        (10, 1.6),
        (11, 1.4),
        (12, 1.2),
        (13, 1.3 + device_bias as f64 * 0.1),
        (14, 1.7),
        (15, 1.9),
        (16, 1.8 + device_bias as f64 * 0.15),
        (17, 1.5),
        (18, 1.0),
        (19, 0.8),
        (20, 0.6),
        (21, 0.5),
        (22, 0.4),
        (23, 0.3),
    ];

    let sessions_per_hour: Vec<PeakHour> = raw_peaks
        .iter()
        .map(|(h, weight)| PeakHour {
            hour: *h,
            session_count: ((total_sessions as f64 / lookback as f64) * weight / 24.0 * 10.0)
                .round() as u64,
        })
        .collect();

    // Return top-5 peak hours sorted by session count descending.
    let mut peak_hours = sessions_per_hour;
    peak_hours.sort_by(|a, b| b.session_count.cmp(&a.session_count));
    peak_hours.truncate(5);

    // ── Retention cohorts ────────────────────────────────────────────────────
    // Generate the most recent 4 cohorts (or fewer if lookback is small).
    let cohort_count = lookback.min(4);
    let mut retention: Vec<RetentionRow> = Vec::with_capacity(cohort_count);

    for c in 0..cohort_count {
        let cohort_start = today - Duration::days(step * (c as i64 + 1));
        let cohort_label = match period {
            ReportPeriod::Monthly => {
                format!("{}-{:02}", cohort_start.year(), cohort_start.month())
            }
            _ => cohort_start.to_string(),
        };

        let cohort_seed = fnv1a_hash(device_id) ^ (c as u64 * 0xCAFE_BABE);
        let new_u = ((base_sessions_per_period as f64) * (0.6 + pseudo_rand(cohort_seed) * 0.4))
            .round() as u64;

        // Retention rate between 20–65 %, influenced by device rating.
        let ret_seed = cohort_seed ^ 0x1234_5678;
        let base_retention = 0.20 + (device.rating / 5.0) * 0.45;
        let ret_rate = (base_retention + pseudo_rand(ret_seed) * 0.10).min(0.99);
        let returning = ((new_u as f64) * ret_rate).round() as u64;

        retention.push(RetentionRow {
            cohort: cohort_label,
            new_users: new_u,
            returning_users: returning,
            retention_rate: round2(ret_rate * 100.0),
        });
    }
    retention.reverse(); // oldest → newest

    let period_label = match period {
        ReportPeriod::Daily => "daily",
        ReportPeriod::Weekly => "weekly",
        ReportPeriod::Monthly => "monthly",
    };

    Some(DeviceAnalyticsReport {
        device_id: device_id.to_string(),
        period: period_label.to_string(),
        total_revenue: round2(total_revenue),
        total_sessions,
        total_unique_users,
        avg_session_duration_secs: round2(avg_session_duration_secs),
        time_series,
        peak_hours,
        retention,
    })
}

/// Render a `DeviceAnalyticsReport` as RFC-4180 CSV.
///
/// Three sections are written in sequence, each preceded by a header comment:
///  1. Summary scalar values
///  2. Time-series table
///  3. Peak hours table
///  4. Retention cohort table
pub fn report_to_csv(report: &DeviceAnalyticsReport) -> Result<String, String> {
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(vec![]);

    // ── Section 1: summary ────────────────────────────────────────────────
    wtr.write_record(["# Summary"]).map_err(|e| e.to_string())?;
    wtr.write_record(["field", "value"])
        .map_err(|e| e.to_string())?;
    wtr.write_record(["device_id", &report.device_id])
        .map_err(|e| e.to_string())?;
    wtr.write_record(["period", &report.period])
        .map_err(|e| e.to_string())?;
    wtr.write_record(["total_revenue_xlm", &report.total_revenue.to_string()])
        .map_err(|e| e.to_string())?;
    wtr.write_record(["total_sessions", &report.total_sessions.to_string()])
        .map_err(|e| e.to_string())?;
    wtr.write_record(["total_unique_users", &report.total_unique_users.to_string()])
        .map_err(|e| e.to_string())?;
    wtr.write_record([
        "avg_session_duration_secs",
        &report.avg_session_duration_secs.to_string(),
    ])
    .map_err(|e| e.to_string())?;
    wtr.write_record([""]).map_err(|e| e.to_string())?;

    // ── Section 2: time-series ────────────────────────────────────────────
    wtr.write_record(["# Time Series"])
        .map_err(|e| e.to_string())?;
    wtr.write_record(["date", "revenue_xlm", "session_count", "unique_users"])
        .map_err(|e| e.to_string())?;
    for p in &report.time_series {
        wtr.write_record([
            &p.date,
            &p.revenue.to_string(),
            &p.session_count.to_string(),
            &p.unique_users.to_string(),
        ])
        .map_err(|e| e.to_string())?;
    }
    wtr.write_record([""]).map_err(|e| e.to_string())?;

    // ── Section 3: peak hours ─────────────────────────────────────────────
    wtr.write_record(["# Peak Hours (UTC)"])
        .map_err(|e| e.to_string())?;
    wtr.write_record(["hour_utc", "session_count"])
        .map_err(|e| e.to_string())?;
    for h in &report.peak_hours {
        wtr.write_record([&h.hour.to_string(), &h.session_count.to_string()])
            .map_err(|e| e.to_string())?;
    }
    wtr.write_record([""]).map_err(|e| e.to_string())?;

    // ── Section 4: retention ──────────────────────────────────────────────
    wtr.write_record(["# Retention Cohorts"])
        .map_err(|e| e.to_string())?;
    wtr.write_record(["cohort", "new_users", "returning_users", "retention_rate_pct"])
        .map_err(|e| e.to_string())?;
    for r in &report.retention {
        wtr.write_record([
            &r.cohort,
            &r.new_users.to_string(),
            &r.returning_users.to_string(),
            &r.retention_rate.to_string(),
        ])
        .map_err(|e| e.to_string())?;
    }

    let bytes = wtr.into_inner().map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

// ─── Utilities ───────────────────────────────────────────────────────────────

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// FNV-1a 64-bit hash for deterministic pseudo-randomness.
fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}
