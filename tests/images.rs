use bitcoincore_rpc::RpcApi;
use bson::{self, doc, Document};
use mongodb::{self, Client};
use redis::Commands;
use rusoto_core::{HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_dynamodb::{
    AttributeDefinition, CreateTableInput, DynamoDb, DynamoDbClient, KeySchemaElement,
    ProvisionedThroughput,
};
use rusoto_sqs::{ListQueuesRequest, Sqs, SqsClient};
use spectral::prelude::*;

use testcontainers::*;

#[test]
fn coblox_bitcoincore_getnewaddress() {
    let _ = pretty_env_logger::try_init();
    let docker = clients::Cli::default();
    let node = docker.run(images::coblox_bitcoincore::BitcoinCore::default());

    let client = {
        let host_port = node.get_host_port(18443).unwrap();

        let url = format!("http://localhost:{}", host_port);

        let auth = node.image().auth();

        bitcoincore_rpc::Client::new(
            url,
            bitcoincore_rpc::Auth::UserPass(auth.username().to_owned(), auth.password().to_owned()),
        )
        .unwrap()
    };

    assert_that(&client.get_new_address(None, None)).is_ok();
}

#[test]
fn parity_parity_net_version() {
    let _ = pretty_env_logger::try_init();
    let docker = clients::Cli::default();
    let node = docker.run(images::parity_parity::ParityEthereum::default());
    let host_port = node.get_host_port(8545).unwrap();

    let mut response = reqwest::Client::new()
        .post(&format!("http://localhost:{}", host_port))
        .body(
            json::object! {
                "jsonrpc" => "2.0",
                "method" => "net_version",
                "params" => json::array![],
                "id" => 1
            }
            .dump(),
        )
        .header("content-type", "application/json")
        .send()
        .unwrap();

    let response = response.text().unwrap();
    let response = json::parse(&response).unwrap();

    assert_eq!(response["result"], "17");
}

#[test]
fn trufflesuite_ganachecli_listaccounts() {
    let _ = pretty_env_logger::try_init();
    let docker = clients::Cli::default();
    let node = docker.run(images::trufflesuite_ganachecli::GanacheCli::default());
    let host_port = node.get_host_port(8545).unwrap();

    let mut response = reqwest::Client::new()
        .post(&format!("http://localhost:{}", host_port))
        .body(
            json::object! {
                "jsonrpc" => "2.0",
                "method" => "net_version",
                "params" => json::array![],
                "id" => 1
            }
            .dump(),
        )
        .header("content-type", "application/json")
        .send()
        .unwrap();

    let response = response.text().unwrap();
    let response = json::parse(&response).unwrap();

    assert_eq!(response["result"], "42");
}

#[test]
fn dynamodb_local_create_table() {
    let _ = pretty_env_logger::try_init();
    let docker = clients::Cli::default();
    let node = docker.run(images::dynamodb_local::DynamoDb::default());
    let host_port = node.get_host_port(8000).unwrap();

    let create_tables_input = CreateTableInput {
        table_name: "books".to_string(),
        key_schema: vec![KeySchemaElement {
            key_type: "HASH".to_string(),
            attribute_name: "title".to_string(),
        }],
        attribute_definitions: vec![AttributeDefinition {
            attribute_name: "title".to_string(),
            attribute_type: "S".to_string(),
        }],
        provisioned_throughput: Some(ProvisionedThroughput {
            read_capacity_units: 5,
            write_capacity_units: 5,
        }),
        ..Default::default()
    };

    let dynamodb = build_dynamodb_client(host_port);
    let result = dynamodb.create_table(create_tables_input).sync();
    assert_that(&result).is_ok();
}

fn build_dynamodb_client(host_port: u16) -> DynamoDbClient {
    let credentials_provider =
        StaticProvider::new("fakeKey".to_string(), "fakeSecret".to_string(), None, None);

    let dispatcher = HttpClient::new().expect("could not create http client");

    let region = Region::Custom {
        name: "dynamodb-local".to_string(),
        endpoint: format!("http://localhost:{}", host_port),
    };

    DynamoDbClient::new_with(dispatcher, credentials_provider, region)
}

#[test]
fn redis_fetch_an_integer() {
    let _ = pretty_env_logger::try_init();
    let docker = clients::Cli::default();
    let node = docker.run(images::redis::Redis::default());
    let host_port = node.get_host_port(6379).unwrap();
    let url = format!("redis://localhost:{}", host_port);

    let client = redis::Client::open(url.as_ref()).unwrap();
    let mut con = client.get_connection().unwrap();

    con.set::<_, _, ()>("my_key", 42).unwrap();
    let result: i64 = con.get("my_key").unwrap();
    assert_eq!(42, result);
}

#[test]
fn mongo_fetch_document() {
    let _ = pretty_env_logger::try_init();
    let docker = clients::Cli::default();
    let node = docker.run(images::mongo::Mongo::default());
    let host_port = node.get_host_port(27017).unwrap();
    let url = format!("mongodb://localhost:{}/", host_port);

    let client: Client = Client::with_uri_str(url.as_ref()).unwrap();
    let db = client.database("some_db");
    let coll = db.collection("some-coll");

    let insert_one_result = coll.insert_one(doc! { "x": 42 }, None).unwrap();
    assert!(!insert_one_result
        .inserted_id
        .as_object_id()
        .unwrap()
        .to_hex()
        .is_empty());

    let find_one_result: Document = coll.find_one(doc! { "x": 42 }, None).unwrap().unwrap();
    assert_eq!(42, find_one_result.get_i32("x").unwrap())
}

#[test]
fn sqs_list_queues() {
    let docker = clients::Cli::default();
    let node = docker.run(images::elasticmq::ElasticMQ::default());
    let host_port = node.get_host_port(9324).unwrap();
    let client = build_sqs_client(host_port);

    let request = ListQueuesRequest::default();
    let result = client.list_queues(request).sync().unwrap();
    assert!(result.queue_urls.is_none());
}

#[test]
fn generic_image() {
    let _ = pretty_env_logger::try_init();
    let docker = clients::Cli::default();

    let db = "postgres-db-test";
    let user = "postgres-user-test";
    let password = "postgres-password-test";

    let generic_postgres = images::generic::GenericImage::new("postgres:9.6-alpine")
        .with_wait_for(images::generic::WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_DB", db)
        .with_env_var("POSTGRES_USER", user)
        .with_env_var("POSTGRES_PASSWORD", password);

    let node = docker.run(generic_postgres);

    let connection_string = &format!(
        "postgres://{}:{}@localhost:{}/{}",
        user,
        password,
        node.get_host_port(5432).unwrap(),
        db
    );
    let mut conn = postgres::Client::connect(connection_string, postgres::NoTls).unwrap();

    let rows = conn.query("SELECT 1 + 1", &[]).unwrap();
    assert_eq!(rows.len(), 1);

    let first_row = &rows[0];
    let first_column: i32 = first_row.get(0);
    assert_eq!(first_column, 2);
}

fn build_sqs_client(host_port: u16) -> SqsClient {
    let dispatcher = HttpClient::new().expect("could not create http client");
    let credentials_provider =
        StaticProvider::new("fakeKey".to_string(), "fakeSecret".to_string(), None, None);
    let region = Region::Custom {
        name: "sqs-local".to_string(),
        endpoint: format!("http://localhost:{}", host_port),
    };

    SqsClient::new_with(dispatcher, credentials_provider, region)
}

#[test]
fn postgres_one_plus_one() {
    let docker = clients::Cli::default();
    let postgres_image = images::postgres::Postgres::default();
    let node = docker.run(postgres_image);

    let connection_string = &format!(
        "postgres://postgres:postgres@localhost:{}/postgres",
        node.get_host_port(5432).unwrap()
    );
    let mut conn = postgres::Client::connect(connection_string, postgres::NoTls).unwrap();

    let rows = conn.query("SELECT 1 + 1", &[]).unwrap();
    assert_eq!(rows.len(), 1);

    let first_row = &rows[0];
    let first_column: i32 = first_row.get(0);
    assert_eq!(first_column, 2);
}

#[test]
fn postgres_one_plus_one_with_custom_mapped_port() {
    let _ = pretty_env_logger::try_init();
    let free_local_port = free_local_port().unwrap();

    let docker = clients::Cli::default();
    let _node =
        docker.run(images::postgres::Postgres::default().with_mapped_port((free_local_port, 5432)));

    let mut conn = postgres::Client::connect(
        &format!(
            "postgres://postgres:postgres@localhost:{}/postgres",
            free_local_port
        ),
        postgres::NoTls,
    )
    .unwrap();
    let rows = conn.query("SELECT 1+1 AS result;", &[]).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<_, i32>("result"), 2);
}

/// Returns an available localhost port
pub fn free_local_port() -> Option<u16> {
    let socket = std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 0);
    std::net::TcpListener::bind(socket)
        .and_then(|listener| listener.local_addr())
        .and_then(|addr| Ok(addr.port()))
        .ok()
}
