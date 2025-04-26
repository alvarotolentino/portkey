use async_trait::async_trait;
use futures::future::join_all;
use serde_json::{Value, json};

use crate::{FederatedSchema, QueryPlan};

#[async_trait]
pub trait QueryExecutor {
    async fn execute_plan(
        &self,
        plan: QueryPlan,
        schema: &FederatedSchema,
    ) -> Result<Value, String>;
}

pub struct HttpQueryExecutor {
    client: reqwest::Client,
}

impl HttpQueryExecutor {
    pub fn new() -> Self {
        HttpQueryExecutor {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl QueryExecutor for HttpQueryExecutor {
    async fn execute_plan(
        &self,
        plan: QueryPlan,
        schema: &FederatedSchema,
    ) -> Result<Value, String> {
        // List of futures for parallel execution
        let mut futures = Vec::new();

        // Create a future for each service query
        for (service_name, query) in plan.service_queries {
            if let Some(service) = schema.services.get(&service_name) {
                let client = self.client.clone();
                let service_url = service.url.clone();
                let query_clone = query.clone();

                // Create a future for this service call
                let future = async move {
                    let request_body = json!({
                        "query": query_clone,
                        "variables": {}
                    });

                    let response = client
                        .post(&service_url)
                        .header("Content-Type", "application/json")
                        .json(&request_body)
                        .send()
                        .await
                        .map_err(|e| {
                            format!("Failed to send request to {}: {}", service_name, e)
                        })?;

                    let json_response = response.json::<Value>().await.map_err(|e| {
                        format!("Failed to parse response from {}: {}", service_name, e)
                    })?;

                    Ok::<(String, Value), String>((service_name, json_response))
                };

                futures.push(future);
            }
        }

        // Execute all service calls in parallel
        let results = join_all(futures).await;

        // Merge results
        let mut merged_data = json!({});

        for result in results {
            match result {
                Ok((service_name, response)) => {
                    // Extract data from response
                    if let Some(data) = response.get("data") {
                        if let Value::Object(fields) = data {
                            for (key, value) in fields {
                                if let Value::Object(ref mut merged_obj) = merged_data["data"] {
                                    merged_obj.insert(key.clone(), value.clone());
                                } else {
                                    merged_data["data"] = json!({ key: value });
                                }
                            }
                        }
                    }

                    // Handle errors
                    if let Some(errors) = response.get("errors") {
                        if let Value::Array(error_list) = errors {
                            if !error_list.is_empty() {
                                // Add service name to errors for debugging
                                let mut service_errors = error_list.clone();
                                for error in &mut service_errors {
                                    if let Value::Object(error_obj) = error {
                                        error_obj.insert(
                                            "service".to_string(),
                                            Value::String(service_name.clone()),
                                        );
                                    }
                                }

                                // Add to merged errors
                                if let Some(Value::Array(merged_errors)) =
                                    merged_data.get_mut("errors")
                                {
                                    merged_errors.extend(service_errors);
                                } else {
                                    merged_data["errors"] = Value::Array(service_errors);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    // Add execution error
                    let error = json!({
                        "message": format!("Execution error: {}", e)
                    });

                    let merged_data_obj = merged_data.as_object_mut().unwrap();
                    if let Some(errors) = merged_data_obj
                        .get_mut("errors")
                        .and_then(|v| v.as_array_mut())
                    {
                        errors.push(error);
                    } else {
                        merged_data_obj.insert("errors".to_string(), Value::Array(vec![error]));
                    }
                }
            }
        }

        Ok(merged_data)
    }
}
