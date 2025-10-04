use std::time::Duration;

pub(crate) fn calculate_bps(duration: Duration, size: usize) -> f64 {
    (size as f64 / duration.as_secs_f64()) * 8.0
}

pub(crate) fn calculate_bandwidth_weight(duration: Duration, size: usize) -> f64 {
    ((size / 5_000_000) as f64) * duration.as_secs_f64()
}

static SPEED_SUFFIX: [&str; 9] = [
    " bps", " Kbps", " Mbps", " Gbps", " Tbps", " Pbps", " Ebps", " Zbps", " Ybps",
];

pub(crate) fn bps_to_string(mut speed: f64) -> String {
    debug_assert!(speed >= 0.0, "speed must be positive");
    let mut order_of_magnitude = 0;
    while speed >= 1_000.0 {
        order_of_magnitude += 1;
        speed /= 1_000.0;
    }
    match speed {
        0.0..10.0 => format!("{:.2}{}", speed, SPEED_SUFFIX[order_of_magnitude]),
        10.0..100.0 => format!("{:.1}{}", speed, SPEED_SUFFIX[order_of_magnitude]),
        _ => format!("{}{}", speed as u64, SPEED_SUFFIX[order_of_magnitude]),
    }
}

pub(crate) fn seconds_to_string(latency: f64) -> String {
    debug_assert!(latency >= 0.0, "speed must be positive");
    let latency_ms = latency * 1_000.0;
    match latency_ms {
        0.0..10.0 => format!("{:.2}ms", latency_ms),
        10.0..100.0 => format!("{:.1}ms", latency_ms),
        _ => format!("{}ms", latency_ms as u64),
    }
}
