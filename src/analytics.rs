use chrono::Utc;
// use lazy_static::lazy_static;
// use reqwest::blocking::Client;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};
use rocket::{Data, Request, Response};
use serde::{Deserialize, Serialize};
// use serde::Serialize;
// use std::sync::Mutex;
// use std::thread::spawn;
use std::time::Instant;
use sha2::{Sha256, Sha512, Digest};

use rocket_db_pools::{Connection, Database, Initializer};

#[derive(Database)]
#[database("sqlx")]
pub struct Db(sqlx::SqlitePool);

impl Db {
    pub fn init() -> Initializer<Self> {
        Initializer::new()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Method {
    GET,
    HEAD,
    POST,
    PUT,
    DELETE,
    CONNECT,
    OPTIONS,
    TRACE,
    PATCH,
    INVALID,
}

impl Method {
    fn to_int(&self) -> u8 {
        match self {
            Method::GET => 1,
            Method::HEAD => 2,
            Method::POST => 3,
            Method::PUT => 4,
            Method::DELETE => 5,
            Method::CONNECT => 6,
            Method::OPTIONS => 7,
            Method::TRACE => 8,
            Method::PATCH => 9,
            Method::INVALID => 10,
        }
    }
    fn from_int(int: u8) -> Method {
        match int {
            1 => Method::GET,
            2 => Method::HEAD,
            3 => Method::POST,
            4 => Method::PUT,
            5 => Method::DELETE,
            6 => Method::CONNECT,
            7 => Method::OPTIONS,
            8 => Method::TRACE,
            9 => Method::PATCH,
            _ => Method::INVALID,
        }
    }
    fn from_text(text: &str) -> Method {
        serde_json::from_str(text).unwrap_or(Method::INVALID)
    }
}

#[derive(Debug, Clone)]
struct RequestData {
    hostname: String,
    ip_address: String,
    path: String,
    user_agent: String,
    method: String,
    status: u16,
    created_at: String,
}

impl RequestData {
    pub fn new(
        hostname: String,
        ip_address: String,
        path: String,
        user_agent: String,
        method: String,
        status: u16,
        created_at: String,
    ) -> Self {
        Self {
            hostname,
            ip_address,
            path,
            user_agent,
            method,
            status,
            created_at,
        }
    }
}

type StringMapper = dyn for<'a, 'r> Fn(&'r Request<'a>, &'r Response) -> String + Send + Sync;

#[derive(Default)]
struct Mappers {
    hostname: Option<Box<StringMapper>>,
    ip_address: Option<Box<StringMapper>>,
    path: Option<Box<StringMapper>>,
}

#[derive(Default)]
pub struct Analytics {
    mappers: Mappers,
}

impl Analytics {
    pub fn new() -> Self {
        Self {
            mappers: Default::default(),
        }
    }
}

async fn log_request(request_data: RequestData, mut conn: Connection<Db>) {
    dbg!(&request_data);

    use std::time::SystemTime;

    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let mut hasher = Sha256::new();

    hasher.update(request_data.ip_address);

    let ip_address_hash = hasher.finalize();

    let ip_address_hash: &[u8] = &ip_address_hash[..];

    let method = Method::from_text(&request_data.method).to_int();

    let result = sqlx::query!(
        "INSERT INTO requests (ip_address_hash, path, method, status, created_at) VALUES($1, $2, $3, $4, $5) RETURNING id",
        ip_address_hash, request_data.path, method, request_data.status, time
    ).fetch_one(&mut **conn)
    .await;

}

#[rocket::async_trait]
impl Fairing for Analytics {
    fn info(&self) -> Info {
        Info {
            name: "API Analytics",
            kind: Kind::Request | Kind::Response,
        }
    }

    async fn on_response<'r>(&self, req: &'r Request<'_>, res: &mut Response<'r>) {
        let hostname = self
            .mappers
            .hostname
            .as_ref()
            .map(|m| m(req, res))
            .unwrap_or_else(|| req.host().unwrap().to_string());
        let ip_address = self
            .mappers
            .ip_address
            .as_ref()
            .map(|m| m(req, res))
            .unwrap_or_else(|| req.client_ip().unwrap().to_string());
        let method = req.method().to_string();
        let user_agent = req
            .headers()
            .get_one("User-Agent")
            .unwrap_or_default()
            .to_owned();
        let path = self
            .mappers
            .path
            .as_ref()
            .map(|m| m(req, res))
            .unwrap_or_else(|| req.uri().path().to_string());

        let request_data = RequestData::new(
            hostname,
            ip_address,
            path,
            user_agent,
            method,
            res.status().code,
            Utc::now().to_rfc3339(),
        );

        let conn = Connection::<Db>::from_request(req)
            .await
            .expect("could not connect to the database");

        log_request(request_data, conn).await;
    }
}



