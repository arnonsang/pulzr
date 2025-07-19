use crate::client::HttpClient;
use crate::debug::DebugConfig;
use crate::endpoints::{Endpoint, MultiEndpointConfig};
use crate::ramp_up::RampUpConfig;
use crate::rate_limiter::RequestRateLimiter;
use crate::scenario::{Scenario, ScenarioStep};
use crate::stats::{RequestResult, StatsCollector};
use anyhow::Result;
use chrono::Utc;
use rand::rng;
use rand_distr::{weighted::WeightedIndex, Distribution};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::time::{sleep, timeout};

pub struct LoadTester {
    client: Arc<HttpClient>,
    rate_limiter: Arc<RequestRateLimiter>,
    stats_collector: Arc<StatsCollector>,
    concurrent_requests: usize,
    test_duration: Option<Duration>,
    total_requests: Option<u64>,
    scenario: Option<Arc<Scenario>>,
    endpoints: Option<Arc<MultiEndpointConfig>>,
    ramp_up: Option<RampUpConfig>,
    debug_config: DebugConfig,
}

impl LoadTester {
    pub fn new(
        client: Arc<HttpClient>,
        rate_limiter: Arc<RequestRateLimiter>,
        stats_collector: Arc<StatsCollector>,
        concurrent_requests: usize,
        test_duration: Option<Duration>,
    ) -> Self {
        Self {
            client,
            rate_limiter,
            stats_collector,
            concurrent_requests,
            test_duration,
            total_requests: None,
            scenario: None,
            endpoints: None,
            ramp_up: None,
            debug_config: DebugConfig::disabled(),
        }
    }

    pub fn with_total_requests(mut self, total_requests: Option<u64>) -> Self {
        self.total_requests = total_requests;
        self
    }

    pub fn with_scenario(mut self, scenario: Arc<Scenario>) -> Self {
        self.scenario = Some(scenario);
        self
    }

    pub fn with_endpoints(mut self, endpoints: Arc<MultiEndpointConfig>) -> Self {
        self.endpoints = Some(endpoints);
        self
    }

    pub fn with_ramp_up(mut self, ramp_up: RampUpConfig) -> Self {
        self.ramp_up = Some(ramp_up);
        self
    }

    pub fn with_debug(mut self, debug_config: DebugConfig) -> Self {
        self.debug_config = debug_config;
        self
    }

    pub async fn run_test(&self, quit_receiver: broadcast::Receiver<()>) -> Result<()> {
        if let Some(ramp_up) = &self.ramp_up {
            self.run_test_with_ramp_up(quit_receiver, ramp_up.clone())
                .await
        } else {
            self.run_test_fixed_concurrency(quit_receiver).await
        }
    }

    async fn run_test_fixed_concurrency(
        &self,
        mut quit_receiver: broadcast::Receiver<()>,
    ) -> Result<()> {
        let mut handles = Vec::new();

        // Shared request counter for total request limit
        let request_counter = Arc::new(AtomicUsize::new(0));

        // Prepare request data (scenarios, endpoints, or single URL)
        let (scenario_data, endpoint_data) = self.prepare_request_data();
        let random_indices = self.generate_random_indices(&scenario_data, &endpoint_data);

        for _ in 0..self.concurrent_requests {
            let client = Arc::clone(&self.client);
            let rate_limiter = Arc::clone(&self.rate_limiter);
            let stats_collector = Arc::clone(&self.stats_collector);
            let test_duration = self.test_duration;
            let total_requests = self.total_requests;
            let request_counter = Arc::clone(&request_counter);
            let scenario = self.scenario.clone();
            let endpoints = self.endpoints.clone();
            let scenario_data = scenario_data.clone();
            let endpoint_data = endpoint_data.clone();
            let random_indices = random_indices.clone();
            let debug_config = self.debug_config.clone();
            let mut quit_rx = quit_receiver.resubscribe();

            let handle = tokio::spawn(async move {
                let end_time = test_duration.map(|d| tokio::time::Instant::now() + d);
                let mut request_count = 0usize;

                loop {
                    tokio::select! {
                        _ = quit_rx.recv() => {
                            break;
                        }
                        _ = async {
                            // Check if we have a duration and if it's exceeded
                            if let Some(end) = end_time {
                                if tokio::time::Instant::now() >= end {
                                    return;
                                }
                            }

                            // Check if we have reached the total request limit
                            if let Some(max_requests) = total_requests {
                                let current_count = request_counter.load(Ordering::SeqCst);
                                if current_count >= max_requests as usize {
                                    return;
                                }
                            }

                            rate_limiter.acquire().await;

                            // Execute based on type: scenario step, endpoint, or regular request
                            if let Some((steps, _)) = &scenario_data {
                                // Scenario-based testing
                                if let Some(scenario) = &scenario {
                                    if let Some(indices) = &random_indices {
                                        let step_index = indices[request_count % indices.len()];
                                        let step = &steps[step_index];
                                        let processed_step = step.substitute_variables(scenario);

                                        if let Err(e) = Self::execute_scenario_step(&client, &processed_step, Arc::clone(&stats_collector), &debug_config).await {
                                            eprintln!("Scenario step '{}' error: {}", step.name, e);
                                        }

                                        request_count += 1;
                                    }
                                }
                            } else if let Some((endpoints_list, _)) = &endpoint_data {
                                // Multi-endpoint testing
                                if let Some(endpoints_config) = &endpoints {
                                    if let Some(indices) = &random_indices {
                                        let endpoint_index = indices[request_count % indices.len()];
                                        let endpoint = &endpoints_list[endpoint_index];

                                        if let Err(e) = Self::execute_endpoint_request(&client, endpoint, &endpoints_config.defaults, Arc::clone(&stats_collector), &debug_config).await {
                                            eprintln!("Endpoint '{}' error: {}", endpoint.name, e);
                                        }

                                        request_count += 1;
                                    }
                                }
                            } else {
                                // Regular single URL request
                                let request_timeout = Duration::from_secs(30);
                                match timeout(request_timeout, client.send_request()).await {
                                    Ok(_) => {},
                                    Err(_) => {
                                        eprintln!("Request timeout");
                                    }
                                }
                                request_counter.fetch_add(1, Ordering::SeqCst);
                            }
                        } => {
                            if let Some(end) = end_time {
                                if tokio::time::Instant::now() >= end {
                                    break;
                                }
                            }
                        }
                    }
                }
            });

            handles.push(handle);
        }

        // Wait for either duration, request count, or quit signal
        if let Some(total_requests) = self.total_requests {
            // Request count mode: wait until all requests are done
            loop {
                tokio::select! {
                    _ = quit_receiver.recv() => {
                        println!("Received quit signal, stopping test...");
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        let current_count = request_counter.load(Ordering::SeqCst);
                        if current_count >= total_requests as usize {
                            println!("Completed {} requests", total_requests);
                            break;
                        }
                    }
                }
            }
        } else if let Some(duration) = self.test_duration {
            tokio::select! {
                _ = sleep(duration) => {},
                _ = quit_receiver.recv() => {
                    println!("Received quit signal, stopping test...");
                }
            }
        } else {
            // Run indefinitely until quit signal
            let _ = quit_receiver.recv().await;
            println!("Received quit signal, stopping test...");
        }

        for handle in handles {
            handle.abort();
        }

        Ok(())
    }

    async fn run_test_with_ramp_up(
        &self,
        mut quit_receiver: broadcast::Receiver<()>,
        ramp_up: RampUpConfig,
    ) -> Result<()> {
        let mut handles = Vec::new();
        let active_tasks = Arc::new(AtomicUsize::new(0));
        let request_counter = Arc::new(AtomicUsize::new(0));
        let start_time = Instant::now();

        // Prepare request data (scenarios, endpoints, or single URL)
        let (scenario_data, endpoint_data) = self.prepare_request_data();
        let random_indices = self.generate_random_indices(&scenario_data, &endpoint_data);

        println!("Starting ramp-up: {}", ramp_up.description());

        // Spawn initial tasks
        let initial_concurrency = ramp_up.current_concurrency(start_time);
        for _ in 0..initial_concurrency {
            let handle = self.spawn_worker_task(
                &mut quit_receiver,
                start_time,
                scenario_data.clone(),
                endpoint_data.clone(),
                random_indices.clone(),
                (Arc::clone(&active_tasks), Arc::clone(&request_counter)),
            );
            handles.push(handle);
            active_tasks.fetch_add(1, Ordering::SeqCst);
        }

        // Monitor and adjust concurrency during ramp-up
        let mut last_concurrency = initial_concurrency;
        let mut check_interval = tokio::time::interval(Duration::from_millis(500));

        loop {
            tokio::select! {
                _ = quit_receiver.recv() => {
                    println!("Received quit signal, stopping ramp-up test...");
                    break;
                }
                _ = check_interval.tick() => {
                    // Check if we have reached the total request limit
                    if let Some(max_requests) = self.total_requests {
                        let current_count = request_counter.load(Ordering::SeqCst);
                        if current_count >= max_requests as usize {
                            println!("Completed {} requests", max_requests);
                            break;
                        }
                    }

                    // Check if test duration is exceeded
                    if let Some(duration) = self.test_duration {
                        if start_time.elapsed() >= duration {
                            break;
                        }
                    }

                    let current_concurrency = ramp_up.current_concurrency(start_time);

                    // Adjust concurrency if needed
                    if current_concurrency != last_concurrency {
                        if current_concurrency > last_concurrency {
                            // Spawn additional tasks
                            for _ in 0..(current_concurrency - last_concurrency) {
                                let handle = self.spawn_worker_task(
                                    &mut quit_receiver,
                                    start_time,
                                    scenario_data.clone(),
                                    endpoint_data.clone(),
                                    random_indices.clone(),
                                    (Arc::clone(&active_tasks), Arc::clone(&request_counter)),
                                );
                                handles.push(handle);
                                active_tasks.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                        // Note: We don't reduce concurrency mid-test to avoid complexity
                        // Tasks will naturally complete and not be replaced

                        last_concurrency = current_concurrency;
                        println!("Ramp-up progress: {current_concurrency} concurrent workers");
                    }

                    // Check if ramp-up is complete
                    if !ramp_up.is_ramping(start_time) && last_concurrency == ramp_up.max_concurrent {
                        println!("Ramp-up complete! Running at {} concurrent requests", ramp_up.max_concurrent);

                        // Continue with remaining test duration if any
                        if let Some(duration) = self.test_duration {
                            let remaining = duration.saturating_sub(start_time.elapsed());
                            if remaining > Duration::from_secs(0) {
                                println!("Continuing test for remaining {remaining:?}");
                                tokio::select! {
                                    _ = sleep(remaining) => {},
                                    _ = quit_receiver.recv() => {
                                        println!("Received quit signal during steady phase");
                                    }
                                }
                            }
                        } else {
                            // Run indefinitely until quit signal
                            let _ = quit_receiver.recv().await;
                            println!("Received quit signal, stopping test...");
                        }
                        break;
                    }
                }
            }
        }

        // Cleanup: abort all tasks
        for handle in handles {
            handle.abort();
        }

        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn prepare_request_data(
        &self,
    ) -> (
        Option<(Vec<ScenarioStep>, Option<WeightedIndex<f64>>)>,
        Option<(Vec<Endpoint>, Option<WeightedIndex<f64>>)>,
    ) {
        let scenario_data = if let Some(scenario) = &self.scenario {
            let weights: Vec<f64> = scenario
                .steps
                .iter()
                .map(|step| step.get_weight())
                .collect();
            let weighted_index = WeightedIndex::new(&weights).ok();
            Some((scenario.steps.clone(), weighted_index))
        } else {
            None
        };

        let endpoint_data = if let Some(endpoints) = &self.endpoints {
            let weights: Vec<f64> = endpoints
                .endpoints
                .iter()
                .map(|endpoint| endpoint.get_weight(&endpoints.defaults))
                .collect();
            let weighted_index = WeightedIndex::new(&weights).ok();
            Some((endpoints.endpoints.clone(), weighted_index))
        } else {
            None
        };

        (scenario_data, endpoint_data)
    }

    fn generate_random_indices(
        &self,
        scenario_data: &Option<(Vec<ScenarioStep>, Option<WeightedIndex<f64>>)>,
        endpoint_data: &Option<(Vec<Endpoint>, Option<WeightedIndex<f64>>)>,
    ) -> Option<Arc<Vec<usize>>> {
        if scenario_data.is_some() || endpoint_data.is_some() {
            let mut rng = rng();
            let indices: Vec<usize> = (0..10000)
                .map(|_| {
                    if let Some((_, Some(weighted_index))) = scenario_data {
                        weighted_index.sample(&mut rng)
                    } else if let Some((_, Some(weighted_index))) = endpoint_data {
                        weighted_index.sample(&mut rng)
                    } else {
                        0
                    }
                })
                .collect();
            Some(Arc::new(indices))
        } else {
            None
        }
    }

    fn spawn_worker_task(
        &self,
        quit_receiver: &mut broadcast::Receiver<()>,
        start_time: Instant,
        scenario_data: Option<(Vec<ScenarioStep>, Option<WeightedIndex<f64>>)>,
        endpoint_data: Option<(Vec<Endpoint>, Option<WeightedIndex<f64>>)>,
        random_indices: Option<Arc<Vec<usize>>>,
        counters: (Arc<AtomicUsize>, Arc<AtomicUsize>), // (active_tasks, request_counter)
    ) -> tokio::task::JoinHandle<()> {
        let client = Arc::clone(&self.client);
        let rate_limiter = Arc::clone(&self.rate_limiter);
        let stats_collector = Arc::clone(&self.stats_collector);
        let test_duration = self.test_duration;
        let total_requests = self.total_requests;
        let scenario = self.scenario.clone();
        let endpoints = self.endpoints.clone();
        let debug_config = self.debug_config.clone();
        let mut quit_rx = quit_receiver.resubscribe();

        let (active_tasks, request_counter) = counters;

        tokio::spawn(async move {
            let end_time = test_duration.map(|d| start_time + d);
            let mut request_count = 0usize;

            loop {
                tokio::select! {
                    _ = quit_rx.recv() => {
                        break;
                    }
                    _ = async {
                        // Check if we have a duration and if it's exceeded
                        if let Some(end) = end_time {
                            if Instant::now() >= end {
                                return;
                            }
                        }

                        // Check if we have reached the total request limit
                        if let Some(max_requests) = total_requests {
                            let current_count = request_counter.load(Ordering::SeqCst);
                            if current_count >= max_requests as usize {
                                return;
                            }
                        }

                        rate_limiter.acquire().await;

                        // Execute based on type: scenario step, endpoint, or regular request
                        if let Some((steps, _)) = &scenario_data {
                            // Scenario-based testing
                            if let Some(scenario) = &scenario {
                                if let Some(indices) = &random_indices {
                                    let step_index = indices[request_count % indices.len()];
                                    let step = &steps[step_index];
                                    let processed_step = step.substitute_variables(scenario);

                                    if let Err(e) = Self::execute_scenario_step(&client, &processed_step, Arc::clone(&stats_collector), &debug_config).await {
                                        eprintln!("Scenario step '{}' error: {}", step.name, e);
                                    }

                                    request_count += 1;
                                    request_counter.fetch_add(1, Ordering::SeqCst);
                                }
                            }
                        } else if let Some((endpoints_list, _)) = &endpoint_data {
                            // Multi-endpoint testing
                            if let Some(endpoints_config) = &endpoints {
                                if let Some(indices) = &random_indices {
                                    let endpoint_index = indices[request_count % indices.len()];
                                    let endpoint = &endpoints_list[endpoint_index];

                                    if let Err(e) = Self::execute_endpoint_request(&client, endpoint, &endpoints_config.defaults, Arc::clone(&stats_collector), &debug_config).await {
                                        eprintln!("Endpoint '{}' error: {}", endpoint.name, e);
                                    }

                                    request_count += 1;
                                    request_counter.fetch_add(1, Ordering::SeqCst);
                                }
                            }
                        } else {
                            // Regular single URL request
                            let request_timeout = Duration::from_secs(30);
                            match timeout(request_timeout, client.send_request()).await {
                                Ok(_) => {},
                                Err(_) => {
                                    eprintln!("Request timeout");
                                }
                            }
                            request_counter.fetch_add(1, Ordering::SeqCst);
                        }
                    } => {
                        if let Some(end) = end_time {
                            if Instant::now() >= end {
                                break;
                            }
                        }
                    }
                }
            }

            // Decrement active task counter when this task ends
            active_tasks.fetch_sub(1, Ordering::SeqCst);
        })
    }

    async fn execute_scenario_step(
        _client: &HttpClient,
        step: &ScenarioStep,
        stats_collector: Arc<StatsCollector>,
        debug_config: &DebugConfig,
    ) -> Result<()> {
        use crate::debug::{DebugConfig, RequestDebugInfo, ResponseDebugInfo};

        // Generate session ID for debug correlation
        let session_id = DebugConfig::generate_session_id();

        // Create a temporary client for this specific step
        let step_client = reqwest::Client::new();
        let mut request = step_client.request(step.get_method(), &step.url);

        // Add headers
        if let Some(headers) = &step.headers {
            for (key, value) in headers {
                request = request.header(key, value);
            }
        }

        // Add payload if present
        if let Some(payload) = &step.payload {
            request = request.body(payload.clone());
        }

        // Set timeout
        let timeout_duration = step.get_timeout().unwrap_or(Duration::from_secs(30));

        // Debug: Log request details
        if debug_config.enabled {
            let request_info = RequestDebugInfo {
                timestamp: chrono::Utc::now(),
                method: step.get_method().to_string(),
                url: step.url.clone(),
                headers: step.headers.clone(),
                body: step.payload.clone(),
                user_agent: None,
            };
            debug_config.log_request(&request_info, &session_id);
        }

        let start_time = std::time::Instant::now();

        match timeout(timeout_duration, request.send()).await {
            Ok(Ok(response)) => {
                let elapsed = start_time.elapsed();
                let status = response.status().as_u16();
                let content_length = response.content_length().unwrap_or(0);

                // Debug: Log response details
                if debug_config.enabled {
                    let response_info = ResponseDebugInfo {
                        timestamp: chrono::Utc::now(),
                        status,
                        headers: None, // Could extract if needed based on debug level
                        body: None,    // Could extract if needed based on debug level
                        content_length: Some(content_length),
                        duration: elapsed,
                    };
                    debug_config.log_response(&response_info, &session_id);
                }

                // Record successful request in stats
                let result = RequestResult {
                    timestamp: Utc::now(),
                    duration_ms: elapsed.as_millis() as u64,
                    status_code: Some(status),
                    error: None,
                    user_agent: None, // Could extract from headers if needed
                    bytes_received: content_length,
                };
                stats_collector.record_request(result).await;

                println!(
                    "Step '{}': {} {} in {:?}",
                    step.name, status, step.url, elapsed
                );
                Ok(())
            }
            Ok(Err(e)) => {
                let elapsed = start_time.elapsed();

                // Debug: Log error details
                if debug_config.enabled {
                    debug_config.log_error(&e.to_string(), &session_id, elapsed);
                }

                // Record failed request in stats
                let result = RequestResult {
                    timestamp: Utc::now(),
                    duration_ms: elapsed.as_millis() as u64,
                    status_code: None,
                    error: Some(e.to_string()),
                    user_agent: None,
                    bytes_received: 0,
                };
                stats_collector.record_request(result).await;

                eprintln!(
                    "Step '{}' request error: {} (took {:?})",
                    step.name, e, elapsed
                );
                Ok(()) // Don't fail the whole test for one request
            }
            Err(_) => {
                let elapsed = timeout_duration;

                // Debug: Log timeout error
                if debug_config.enabled {
                    debug_config.log_error("Request timeout", &session_id, elapsed);
                }

                // Record timeout as failed request
                let result = RequestResult {
                    timestamp: Utc::now(),
                    duration_ms: elapsed.as_millis() as u64,
                    status_code: None,
                    error: Some("Request timeout".to_string()),
                    user_agent: None,
                    bytes_received: 0,
                };
                stats_collector.record_request(result).await;

                eprintln!("Step '{}' timeout after {:?}", step.name, timeout_duration);
                Ok(()) // Don't fail the whole test for timeouts
            }
        }
    }

    async fn execute_endpoint_request(
        _client: &HttpClient,
        endpoint: &Endpoint,
        defaults: &Option<crate::endpoints::EndpointDefaults>,
        stats_collector: Arc<StatsCollector>,
        debug_config: &DebugConfig,
    ) -> Result<()> {
        use crate::debug::{DebugConfig, RequestDebugInfo, ResponseDebugInfo};

        // Generate session ID for debug correlation
        let session_id = DebugConfig::generate_session_id();

        // Create a client for this specific endpoint
        let endpoint_client = reqwest::Client::new();
        let mut request = endpoint_client.request(endpoint.get_method(defaults), &endpoint.url);

        // Add headers
        let headers = endpoint.get_headers(defaults);
        for (key, value) in &headers {
            request = request.header(key, value);
        }

        // Add payload if present
        if let Some(payload) = &endpoint.payload {
            request = request.body(payload.clone());
        }

        // Set timeout
        let timeout_duration = endpoint
            .get_timeout(defaults)
            .unwrap_or(Duration::from_secs(30));

        // Debug: Log request details
        if debug_config.enabled {
            let headers_map = if debug_config.level >= crate::debug::DebugLevel::Headers {
                Some(headers.into_iter().collect())
            } else {
                None
            };

            let request_info = RequestDebugInfo {
                timestamp: chrono::Utc::now(),
                method: endpoint.get_method(defaults).to_string(),
                url: endpoint.url.clone(),
                headers: headers_map,
                body: if debug_config.level >= crate::debug::DebugLevel::Full {
                    endpoint.payload.clone()
                } else {
                    None
                },
                user_agent: None,
            };
            debug_config.log_request(&request_info, &session_id);
        }

        let start_time = std::time::Instant::now();

        match timeout(timeout_duration, request.send()).await {
            Ok(Ok(response)) => {
                let elapsed = start_time.elapsed();
                let status = response.status().as_u16();
                let content_length = response.content_length().unwrap_or(0);

                // Debug: Log response details
                if debug_config.enabled {
                    let response_info = ResponseDebugInfo {
                        timestamp: chrono::Utc::now(),
                        status,
                        headers: None, // Could extract if needed based on debug level
                        body: None,    // Could extract if needed based on debug level
                        content_length: Some(content_length),
                        duration: elapsed,
                    };
                    debug_config.log_response(&response_info, &session_id);
                }

                // Check if status is expected for this endpoint
                let is_success = endpoint.is_expected_status(status);

                // Record request in stats
                let result = RequestResult {
                    timestamp: Utc::now(),
                    duration_ms: elapsed.as_millis() as u64,
                    status_code: Some(status),
                    error: if is_success {
                        None
                    } else {
                        Some(format!("Unexpected status: {status}"))
                    },
                    user_agent: None,
                    bytes_received: content_length,
                };
                stats_collector.record_request(result).await;

                println!(
                    "Endpoint '{}': {} {} in {:?} {}",
                    endpoint.name,
                    status,
                    endpoint.url,
                    elapsed,
                    if is_success { "✓" } else { "✗" }
                );
                Ok(())
            }
            Ok(Err(e)) => {
                let elapsed = start_time.elapsed();

                // Debug: Log error details
                if debug_config.enabled {
                    debug_config.log_error(&e.to_string(), &session_id, elapsed);
                }

                // Record failed request in stats
                let result = RequestResult {
                    timestamp: Utc::now(),
                    duration_ms: elapsed.as_millis() as u64,
                    status_code: None,
                    error: Some(e.to_string()),
                    user_agent: None,
                    bytes_received: 0,
                };
                stats_collector.record_request(result).await;

                eprintln!(
                    "Endpoint '{}' request error: {} (took {:?})",
                    endpoint.name, e, elapsed
                );
                Ok(()) // Don't fail the whole test for one request
            }
            Err(_) => {
                let elapsed = timeout_duration;

                // Debug: Log timeout error
                if debug_config.enabled {
                    debug_config.log_error("Request timeout", &session_id, elapsed);
                }

                // Record timeout as failed request
                let result = RequestResult {
                    timestamp: Utc::now(),
                    duration_ms: elapsed.as_millis() as u64,
                    status_code: None,
                    error: Some("Request timeout".to_string()),
                    user_agent: None,
                    bytes_received: 0,
                };
                stats_collector.record_request(result).await;

                eprintln!(
                    "Endpoint '{}' timeout after {:?}",
                    endpoint.name, timeout_duration
                );
                Ok(()) // Don't fail the whole test for timeouts
            }
        }
    }
}
