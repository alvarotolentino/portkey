use portkey::{
    ServiceConfig, federation_gateway::FederationGateway, query_executor::HttpQueryExecutor,
    query_planner::SimpleQueryPlanner, schema_registry::InMemorySchemaRegistry,
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use serial_test::serial;
use std::fs;
use std::path::Path;
use std::sync::Once;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

// Build Docker images only once across test runs
static DOCKER_BUILD: Once = Once::new();

// Test fixture to manage test resources and setup
struct TestFixture {
    gateway: FederationGateway,
    user_container: ContainerAsync<GenericImage>,
    product_container: ContainerAsync<GenericImage>,
    user_id: Option<String>,
    product_id: Option<String>,
}

impl TestFixture {
    // Create and initialize the test environment
    async fn setup() -> Result<Self, Box<dyn std::error::Error>> {
        // Build the docker images (only once)
        TestFixture::build_docker_images();

        // Start containers
        let (user_container, product_container) = TestFixture::start_containers().await?;

        // Get service URLs
        let user_port = user_container.get_host_port_ipv4(4000).await.unwrap();
        let product_port = product_container.get_host_port_ipv4(4001).await.unwrap();

        let user_service_url = format!("http://localhost:{}/graphql", user_port);
        let product_service_url = format!("http://localhost:{}/graphql", product_port);

        println!("User service running at: {}", user_service_url);
        println!("Product service running at: {}", product_service_url);

        // Build the federation gateway
        let gateway = TestFixture::build_gateway(&user_service_url, &product_service_url).await;

        // Wait to ensure services are fully ready
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        Ok(Self {
            gateway,
            user_container,
            product_container,
            user_id: None,
            product_id: None,
        })
    }

    // Build Docker images for test services
    fn build_docker_images() {
        DOCKER_BUILD.call_once(|| {
            println!("Building Docker images for test services...");
            let user_service_dir = Path::new("docker/user-service").canonicalize().unwrap();
            let product_service_dir = Path::new("docker/product-service").canonicalize().unwrap();

            // Build user service image
            std::process::Command::new("docker")
                .arg("build")
                .arg("-t")
                .arg("user-service:latest")
                .arg(user_service_dir)
                .status()
                .expect("Failed to build user service image");

            // Build product service image
            std::process::Command::new("docker")
                .arg("build")
                .arg("-t")
                .arg("product-service:latest")
                .arg(product_service_dir)
                .status()
                .expect("Failed to build product service image");

            println!("Docker images built successfully");
        });
    }

    // Start service containers
    async fn start_containers() -> Result<
        (ContainerAsync<GenericImage>, ContainerAsync<GenericImage>),
        Box<dyn std::error::Error>,
    > {
        println!("Starting service containers...");

        let user_container = GenericImage::new("user-service", "latest")
            .with_exposed_port(4000.tcp())
            .with_wait_for(WaitFor::message_on_stdout("🚀 User service ready at"))
            .with_network("bridge")
            .start()
            .await
            .expect("Failed to start user service");

        println!("User service container started");

        let product_container: testcontainers::ContainerAsync<GenericImage> =
            GenericImage::new("product-service", "latest")
                .with_exposed_port(4001.tcp())
                .with_wait_for(WaitFor::message_on_stdout("🚀 Product service ready at"))
                .with_network("bridge")
                .start()
                .await
                .expect("Failed to start product service");

        println!("Product service container started");

        Ok((user_container, product_container))
    }

    // Build the federation gateway
    async fn build_gateway(user_service_url: &str, product_service_url: &str) -> FederationGateway {
        // Read the schema files
        let user_schema = fs::read_to_string(Path::new("schemas/service_1.graphql"))
            .expect("Could not read user service schema");
        let product_schema = fs::read_to_string(Path::new("schemas/service_2.graphql"))
            .expect("Could not read product service schema");

        // Create a schema registry
        let schema_registry = Box::new(InMemorySchemaRegistry::new());
        let query_planner = Box::new(SimpleQueryPlanner::new());
        let query_executor = Box::new(HttpQueryExecutor::new());

        let gateway = FederationGateway::new(schema_registry, query_planner, query_executor);

        // Register the services
        let user_service = ServiceConfig {
            name: "service_1".to_string(),
            url: user_service_url.to_string(),
            schema: user_schema,
        };

        let product_service = ServiceConfig {
            name: "service_2".to_string(),
            url: product_service_url.to_string(),
            schema: product_schema,
        };

        gateway.register_service(user_service).await.unwrap();
        gateway.register_service(product_service).await.unwrap();

        gateway
    }

    // Helper method to execute GraphQL queries
    async fn execute_query(&self, query: &str, variables: Option<Value>) -> Result<Value, String> {
        let request = portkey::GraphQLRequest {
            query: query.to_string(),
            variables,
            operation_name: None,
            auth_headers: None,
        };

        self.gateway.process_request(request).await
    }
}

#[tokio::test]
#[serial]
async fn test_federated_queries() -> Result<(), Box<dyn std::error::Error>> {
    // Setup test environment
    println!("Setting up test environment...");
    let mut fixture = TestFixture::setup().await?;

    println!("Test environment ready");

    // Test 1: Query users
    println!("Running Test 1: Query users");
    let query_users = r#"
    query {
      users {
        id
        name
        email
      }
    }
    "#;

    let result = fixture.execute_query(query_users, None).await?;
    assert!(result["data"]["users"].is_array());
    assert_eq!(result["data"]["users"].as_array().unwrap().len(), 2);

    // Test 2: Query products
    println!("Running Test 2: Query products");
    let query_products = r#"
    query {
      products {
        id
        name
        price
      }
    }
    "#;

    let result = fixture.execute_query(query_products, None).await?;
    assert!(result["data"]["products"].is_array());
    assert_eq!(result["data"]["products"].as_array().unwrap().len(), 2);

    // Test 3: Query by ID
    println!("Running Test 3: Query user by ID");
    let query_by_id = r#"
    query($userId: ID!) {
      user(id: $userId) {
        id
        name
        email
      }
    }
    "#;

    let variables = json!({
        "userId": "1"
    });

    let result = fixture.execute_query(query_by_id, Some(variables)).await?;
    assert_eq!(result["data"]["user"]["id"], "1");
    assert_eq!(result["data"]["user"]["name"], "John Doe");

    // Test 4: Create a new user
    println!("Running Test 4: Create a new user");
    let create_user = r#"
    mutation($input: CreateUserInput!) {
      createUser(input: $input) {
        id
        name
        email
      }
    }
    "#;

    let variables = json!({
        "input": {
            "name": "Alice Brown",
            "email": "alice@example.com"
        }
    });

    let result = fixture.execute_query(create_user, Some(variables)).await?;
    assert_eq!(result["data"]["createUser"]["name"], "Alice Brown");
    assert_eq!(result["data"]["createUser"]["email"], "alice@example.com");
    fixture.user_id = Some(
        result["data"]["createUser"]["id"]
            .as_str()
            .unwrap()
            .to_string(),
    );

    // Test 5: Create a new product
    println!("Running Test 5: Create a new product");
    let create_product = r#"
    mutation($input: CreateProductInput!) {
      createProduct(input: $input) {
        id
        name
        price
      }
    }
    "#;

    let variables = json!({
        "input": {
            "name": "Headphones",
            "price": 149.99
        }
    });

    let result = fixture
        .execute_query(create_product, Some(variables))
        .await?;
    assert_eq!(result["data"]["createProduct"]["name"], "Headphones");
    assert_eq!(result["data"]["createProduct"]["price"], 149.99);
    fixture.product_id = Some(
        result["data"]["createProduct"]["id"]
            .as_str()
            .unwrap()
            .to_string(),
    );

    // Test 6: Combined query (fetching both users and products)
    println!("Running Test 6: Combined query");
    let combined_query = r#"
    query {
      users {
        id
        name
      }
      products {
        id
        name
        price
      }
    }
    "#;

    let result = fixture.execute_query(combined_query, None).await?;
    assert!(result["data"]["users"].is_array());
    assert!(result["data"]["products"].is_array());
    assert!(result["data"]["users"].as_array().unwrap().len() >= 3); // Including the new user
    assert!(result["data"]["products"].as_array().unwrap().len() >= 3); // Including the new product

    // Test 7: Delete a user
    println!("Running Test 7: Delete user");
    let delete_user = r#"
    mutation($id: ID!) {
      deleteUser(id: $id) {
        success
        message
      }
    }
    "#;

    let variables = json!({
        "id": fixture.user_id.as_ref().unwrap()
    });

    let result = fixture.execute_query(delete_user, Some(variables)).await?;
    assert_eq!(result["data"]["deleteUser"]["success"], true);

    // Test 8: Delete a product
    println!("Running Test 8: Delete product");
    let delete_product = r#"
    mutation($id: ID!) {
      deleteProduct(id: $id) {
        success
        message
      }
    }
    "#;

    let variables = json!({
        "id": fixture.product_id.as_ref().unwrap()
    });

    let result = fixture
        .execute_query(delete_product, Some(variables))
        .await?;
    assert_eq!(result["data"]["deleteProduct"]["success"], true);

    println!("All tests completed successfully");
    Ok(())
}
