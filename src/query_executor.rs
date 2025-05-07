use async_trait::async_trait;
use futures::{FutureExt, future::try_join_all};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::{FederatedSchema, QueryPlan};

#[async_trait]
pub trait QueryExecutor: Send + Sync {
    async fn execute_plan(
        &self,
        plan: QueryPlan,
        schema: &FederatedSchema,
        auth_headers: Option<HashMap<String, String>>,
    ) -> Result<Value, String>;
}

pub struct HttpQueryExecutor {}

impl HttpQueryExecutor {
    pub fn new() -> Self {
        HttpQueryExecutor {}
    }
}

#[async_trait]
impl QueryExecutor for HttpQueryExecutor {
    async fn execute_plan(
        &self,
        query_plan: QueryPlan,
        schema: &FederatedSchema,
        auth_headers: Option<HashMap<String, String>>,
    ) -> Result<Value, String> {
        let client = reqwest::Client::new();

        let futures = query_plan
            .service_queries
            .into_iter()
            .map(|(service_name, query)| {
                let service = match schema.services.get(&service_name) {
                    Some(service) => service,
                    None => {
                        return futures::future::ready(Err(format!(
                            "Service not found: {}",
                            service_name
                        )))
                        .left_future();
                    }
                };

                let variables = query_plan
                    .service_variables
                    .get(&service_name)
                    .cloned()
                    .unwrap_or(json!({}));

                println!("Executing query for service: {}", service_name);
                println!("Query: {}", query);
                println!("Variables for service: {}", variables);

                let mut request_builder = client.post(&service.url).json(&json!({
                    "query": query,
                    "variables": variables
                }));

                if let Some(headers) = &auth_headers {
                    for (name, value) in headers {
                        request_builder = request_builder.header(name, value);
                    }
                    println!("Forwarding auth headers to service {}", service_name);
                }

                let request = request_builder.send();

                async move {
                    let response = request
                        .await
                        .map_err(|e| format!("HTTP request failed: {}", e))?;

                    if !response.status().is_success() {
                        let status = response.status();
                        let error_text = response
                            .text()
                            .await
                            .unwrap_or_else(|_| "Could not read error response".to_string());
                        return Err(format!("Service returned error {}: {}", status, error_text));
                    }

                    let response_json = response
                        .json::<Value>()
                        .await
                        .map_err(|e| format!("Failed to parse response: {}", e))?;

                    if let Some(errors) = response_json.get("errors") {
                        println!(
                            "Service {} returned GraphQL errors: {}",
                            service_name, errors
                        );
                    }

                    Ok((service_name, response_json))
                }
                .right_future()
            });

        let results = try_join_all(futures).await?;

        let mut data_map = serde_json::Map::new();
        let mut all_errors = Vec::new();

        for (_service_name, result) in results {
            if let Some(data) = result.get("data").and_then(Value::as_object) {
                data_map.extend(data.clone());
            }

            if let Some(errors) = result.get("errors").and_then(Value::as_array) {
                for error in errors {
                    all_errors.push(error.clone());
                }
            }
        }

        let mut response = json!({"data": data_map});

        if !all_errors.is_empty() {
            response["errors"] = Value::Array(all_errors);
        }

        Ok(response)
    }
}
