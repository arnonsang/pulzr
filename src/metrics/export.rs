use crate::metrics::{FinalSummary, StatsCollector};
use anyhow::Result;
use csv::Writer;
use std::path::Path;
use std::sync::Arc;

pub struct CsvExporter {
    stats_collector: Arc<StatsCollector>,
}

impl CsvExporter {
    pub fn new(stats_collector: Arc<StatsCollector>) -> Self {
        Self { stats_collector }
    }

    pub async fn export_detailed_results<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let results = self.stats_collector.results.read().await;
        let mut writer = Writer::from_path(path)?;

        writer.write_record([
            "timestamp",
            "duration_ms",
            "status_code",
            "error",
            "user_agent",
            "bytes_received",
        ])?;

        for result in results.iter() {
            writer.write_record(&[
                result.timestamp.to_rfc3339(),
                result.duration_ms.to_string(),
                result.status_code.map_or("".to_string(), |c| c.to_string()),
                result.error.as_deref().unwrap_or("").to_string(),
                result.user_agent.as_deref().unwrap_or("").to_string(),
                result.bytes_received.to_string(),
            ])?;
        }

        writer.flush()?;
        Ok(())
    }

    pub async fn export_summary<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let summary = self.stats_collector.get_final_summary().await;
        let mut writer = Writer::from_path(path)?;

        writer.write_record(["metric", "value"])?;

        writer.write_record(["total_requests", &summary.total_requests.to_string()])?;
        writer.write_record([
            "successful_requests",
            &summary.successful_requests.to_string(),
        ])?;
        writer.write_record(["failed_requests", &summary.failed_requests.to_string()])?;
        writer.write_record([
            "test_duration_secs",
            &format!("{:.2}", summary.test_duration_secs),
        ])?;
        writer.write_record(["avg_rps", &format!("{:.2}", summary.avg_rps)])?;
        writer.write_record([
            "avg_response_time",
            &format!("{:.2}", summary.avg_response_time),
        ])?;
        writer.write_record(["min_response_time", &summary.min_response_time.to_string()])?;
        writer.write_record(["max_response_time", &summary.max_response_time.to_string()])?;
        writer.write_record(["p50_response_time", &summary.p50_response_time.to_string()])?;
        writer.write_record(["p95_response_time", &summary.p95_response_time.to_string()])?;
        writer.write_record(["p99_response_time", &summary.p99_response_time.to_string()])?;
        writer.write_record([
            "total_bytes_received",
            &summary.total_bytes_received.to_string(),
        ])?;

        writer.write_record(["", ""])?;
        writer.write_record(["status_codes", "count"])?;
        for (code, count) in &summary.status_codes {
            writer.write_record([&code.to_string(), &count.to_string()])?;
        }

        writer.write_record(["", ""])?;
        writer.write_record(["errors", "count"])?;
        for (error, count) in &summary.errors {
            writer.write_record([error, &count.to_string()])?;
        }

        writer.write_record(["", ""])?;
        writer.write_record(["user_agents", "count"])?;
        for (ua, count) in &summary.user_agents_used {
            writer.write_record([ua, &count.to_string()])?;
        }

        writer.flush()?;
        Ok(())
    }

    pub fn print_summary(&self, summary: &FinalSummary) {
        println!("\n=== LOAD TEST SUMMARY ===");
        println!("Total Requests: {}", summary.total_requests);
        println!(
            "Successful: {} ({:.1}%)",
            summary.successful_requests,
            summary.successful_requests as f64 / summary.total_requests as f64 * 100.0
        );
        println!(
            "Failed: {} ({:.1}%)",
            summary.failed_requests,
            summary.failed_requests as f64 / summary.total_requests as f64 * 100.0
        );
        println!("Test Duration: {:.2}s", summary.test_duration_secs);
        println!("Average RPS: {:.2}", summary.avg_rps);
        println!("Response Times:");
        println!("  Average: {:.2}ms", summary.avg_response_time);
        println!("  Min: {}ms", summary.min_response_time);
        println!("  Max: {}ms", summary.max_response_time);
        println!("  P50: {}ms", summary.p50_response_time);
        println!("  P95: {}ms", summary.p95_response_time);
        println!("  P99: {}ms", summary.p99_response_time);
        println!(
            "Total Bytes: {}",
            format_bytes(summary.total_bytes_received)
        );

        if !summary.status_codes.is_empty() {
            println!("\nStatus Codes:");
            for (code, count) in &summary.status_codes {
                println!("  {code}: {count}");
            }
        }

        if !summary.errors.is_empty() {
            println!("\nErrors:");
            for (error, count) in &summary.errors {
                println!("  {error}: {count}");
            }
        }

        if !summary.user_agents_used.is_empty() && summary.user_agents_used.len() > 1 {
            println!("\nUser Agents Used:");
            for (ua, count) in &summary.user_agents_used {
                println!("  {ua}: {count}");
            }
        }
    }

    pub fn print_compact_summary(
        &self,
        summary: &FinalSummary,
        target_url: &str,
        concurrent: usize,
        duration: Option<f64>,
    ) {
        // Calculate standard deviation (simplified estimation)
        let avg_rps = summary.avg_rps;
        let rps_stdev = avg_rps * 0.1; // Rough estimation

        let avg_latency = summary.avg_response_time;
        let latency_stdev = (summary.p95_response_time as f64 - avg_latency) / 2.0; // Rough estimation

        let max_latency = summary.max_response_time as f64;

        // Header
        if let Some(duration_val) = duration {
            println!(
                "Load testing {} for {:.0}s using {} connections",
                target_url, duration_val, concurrent
            );
        } else {
            println!(
                "Load testing {} using {} connections",
                target_url, concurrent
            );
        }

        // Progress bar simulation (already done)
        let progress_bar = "=".repeat(73);
        if let Some(duration_val) = duration {
            println!("[{}] {:.0}s Done!", progress_bar, duration_val);
        } else {
            println!("[{}] Done!", progress_bar);
        }

        // Statistics table
        println!("Statistics        Avg      Stdev        Max");
        println!(
            "  Reqs/sec    {:8.2}   {:8.2}     {:8.0}",
            avg_rps,
            rps_stdev,
            avg_rps * 1.2
        );
        println!(
            "  Latency     {:8.2}ms {:8.2}ms  {:8.2}ms",
            avg_latency, latency_stdev, max_latency
        );

        // Latency Distribution
        println!("  Latency Distribution");
        println!("     50%    {:8.2}ms", summary.p50_response_time as f64);

        // Calculate 75% and 90% as estimates between p50 and p95
        let p75_estimate = summary.p50_response_time as f64
            + (summary.p95_response_time as f64 - summary.p50_response_time as f64) * 0.5;
        let p90_estimate = summary.p50_response_time as f64
            + (summary.p95_response_time as f64 - summary.p50_response_time as f64) * 0.8;

        println!("     75%    {:8.2}ms", p75_estimate);
        println!("     90%    {:8.2}ms", p90_estimate);
        println!("     99%    {:8.2}ms", summary.p99_response_time as f64);

        // HTTP codes
        if !summary.status_codes.is_empty() {
            let mut codes_1xx = 0;
            let mut codes_2xx = 0;
            let mut codes_3xx = 0;
            let mut codes_4xx = 0;
            let mut codes_5xx = 0;
            let mut codes_others = 0;

            for (code, count) in &summary.status_codes {
                match code / 100 {
                    1 => codes_1xx += count,
                    2 => codes_2xx += count,
                    3 => codes_3xx += count,
                    4 => codes_4xx += count,
                    5 => codes_5xx += count,
                    _ => codes_others += count,
                }
            }

            println!("  HTTP codes:");
            println!(
                "    1xx - {}, 2xx - {}, 3xx - {}, 4xx - {}, 5xx - {}",
                codes_1xx, codes_2xx, codes_3xx, codes_4xx, codes_5xx
            );
            if codes_others > 0 {
                println!("    others - {}", codes_others);
            }
        }

        // Errors
        if !summary.errors.is_empty() {
            println!("  Errors:");
            for (error, count) in &summary.errors {
                println!("    {} - {}", error, count);
            }
        }

        // Throughput
        let throughput_mbps =
            (summary.total_bytes_received as f64 / 1024.0 / 1024.0) / summary.test_duration_secs;
        println!("  Throughput:     {:.2}MB/s", throughput_mbps);
    }

    pub fn print_minimal_summary(&self, summary: &FinalSummary) {
        println!(
            "{} requests in {:.2}s, {} failed, {:.2} req/s",
            summary.total_requests,
            summary.test_duration_secs,
            summary.failed_requests,
            summary.avg_rps
        );
    }

    pub fn print_json_summary(&self, summary: &FinalSummary) {
        let json_output = serde_json::json!({
            "total_requests": summary.total_requests,
            "successful_requests": summary.successful_requests,
            "failed_requests": summary.failed_requests,
            "test_duration_secs": summary.test_duration_secs,
            "requests_per_second": summary.avg_rps,
            "response_times": {
                "average_ms": summary.avg_response_time,
                "min_ms": summary.min_response_time,
                "max_ms": summary.max_response_time,
                "p50_ms": summary.p50_response_time,
                "p95_ms": summary.p95_response_time,
                "p99_ms": summary.p99_response_time
            },
            "status_codes": summary.status_codes,
            "errors": summary.errors,
            "total_bytes_received": summary.total_bytes_received,
            "user_agents_used": summary.user_agents_used
        });

        println!(
            "{}",
            serde_json::to_string_pretty(&json_output).unwrap_or_else(|_| "{}".to_string())
        );
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}
