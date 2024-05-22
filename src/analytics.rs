use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Status;
// use rocket::http::Status;
use rocket::request::FromRequest;
// use rocket::response::{Flash, Redirect};
use rocket::serde::json::Json;
// use rocket::request::{FromRequest, Outcome};
use rocket::{Request, Response};
use rocket_dyn_templates::{context, Template};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use rocket::response::Redirect;

use rocket_db_pools::{Connection, Database, Initializer};
use sqlx::{Acquire, Error};

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
    ip_address: String,
    path: String,
    user_agent: String,
    method: String,
    status: u16,
}

impl RequestData {
    pub fn new(
        ip_address: String,
        path: String,
        user_agent: String,
        method: String,
        status: u16,
    ) -> Self {
        Self {
            ip_address,
            path,
            user_agent,
            method,
            status,
        }
    }
}

type StringMapper = dyn for<'a, 'r> Fn(&'r Request<'a>, &'r Response) -> String + Send + Sync;

#[derive(Default)]
struct Mappers {
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

#[derive(Serialize, Debug)]
pub struct Graphnode {
    pub amount: u32,
    pub timestamp_start: i64,
    pub timestamp_end: i64
}

#[derive(Serialize, Debug)]
pub struct GraphView {
    pub timeline: Vec<Graphnode>,
    pub title: String,
}

async fn get_graph(conn: &mut Connection<Db>, path_id: i32, duration: i64, title: String) -> Result<GraphView, Error> {
    let cap = 20;
    use std::time::SystemTime;

    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let mut list = Vec::<Graphnode>::with_capacity(cap);

    for i in (0..cap as i64).rev() {
        let range_recent = time - (duration * i);
        let range_oldest = time - (duration * (i+1));

        let amount = sqlx::query!(
            "SELECT COUNT(1) as count FROM requests WHERE path_id = $1 AND created_at <= $2  AND created_at > $3",
            path_id, range_recent, range_oldest
        )
        .fetch_one(&mut ***conn)
        .await?;

        list.push(Graphnode{ amount: amount.count as u32, timestamp_start: range_oldest, timestamp_end: range_recent });
    }

    return Ok(GraphView{timeline: list, title});
}

async fn log_request(request_data: RequestData, conn: Connection<Db>) {

    use std::time::SystemTime;

    let time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let mut hasher = Sha256::new();

    // dbg!(&request_data.ip_address);

    hasher.update(request_data.ip_address);

    let ip_address_hash = hasher.finalize();

    let ip_address_hash: &[u8] = &ip_address_hash[..];

    let method = Method::from_text(&request_data.method).to_int();

    let mut val = conn.into_inner();

    let mut transaction = val.begin().await.unwrap();

    let path_id = sqlx::query!(
        "Select path_id FROM paths WHERE path = $1",
        request_data.path
    )
    .fetch_optional(&mut *transaction)
    .await;

    let path_id = match path_id.unwrap() {
        Some(x) => {
            x.path_id
        }
        None => {
            let result =
            sqlx::query!(
                "INSERT INTO paths (path, unique_visitors, total_requests) VALUES($1, $2, $3) RETURNING path_id",
                request_data.path, 0, 0
            )
                .fetch_one(&mut *transaction)
                .await;
            result.unwrap().path_id
        },
    };

    let _result = sqlx::query!(
        "UPDATE paths SET total_requests = total_requests + 1 WHERE path = $1",
        request_data.path
    )
    .execute(&mut *transaction)
    .await;

    let visitor_id = sqlx::query!(
        "SELECT visitor_id FROM visitors WHERE ip_address_hash = $1 LIMIT 1",
        ip_address_hash
    )
    .fetch_optional(&mut *transaction)
    .await.expect("database error");

    let visitor_id = match visitor_id {
        Some(x) => x.visitor_id,
        None => {
            sqlx::query!(
                "INSERT INTO visitors (ip_address_hash) VALUES($1) RETURNING visitor_id",
                ip_address_hash
            )
                .fetch_one(&mut *transaction)
                .await.expect("database error").visitor_id
        },
    };

    let unique_result = sqlx::query!(
        "SELECT id FROM requests WHERE visitor_id = $1 AND path_id = $2 LIMIT 1",
        visitor_id, path_id
    )
    .fetch_optional(&mut *transaction)
    .await;

    let unique = unique_result.expect("analytics database failure").is_none();

    if unique {
        let _result = sqlx::query!(
            "UPDATE paths SET unique_visitors = unique_visitors + 1 WHERE path = $1",
            request_data.path
        )
        .execute(&mut *transaction)
        .await;
    }

    let _result = sqlx::query!(
        "INSERT INTO requests (visitor_id, path_id, user_agent, method, status, created_at) VALUES($1, $2, $3, $4, $5, $6) RETURNING id",
        visitor_id, path_id, request_data.user_agent, method, request_data.status, time
    ).fetch_one(&mut *transaction)
    .await;

    let _x = transaction.commit().await;
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

        let request_data =
            RequestData::new(ip_address, path, user_agent, method, res.status().code);

        let conn = Connection::<Db>::from_request(req)
            .await
            .expect("could not connect to the database");

        log_request(request_data, conn).await;
    }
}
// use rocket::response::content;
// use rocket_contrib::json;
#[derive(Serialize)]
struct Visits {
    pub path_id: i64,
    pub path: String,
    pub unique_visitors: i64,
    pub total_requests: i64,
}

#[get("/visits/id/<id>")]
async fn visits_id(mut db: Connection<Db>, id: i32) -> Result<Json<Visits>, Status> {

    let unique_result =
        sqlx::query_as!(Visits, "SELECT * FROM paths WHERE path_id = $1 LIMIT 1", id)
            .fetch_optional(&mut **db)
            .await;

    match unique_result {
        Ok(x) => match x {
            Some(x) => {
                use rocket::serde::json::Json;
                Ok(Json(x))
            }
            None => Err(Status::NotFound),
        },
        Err(x) => {
            dbg!(x);
            Err(Status::InternalServerError)
        }
    }
}

#[get("/visits/path/<path>")]
async fn visits_path(mut db: Connection<Db>, path: String) -> Result<Json<Visits>, Status> {
    use rocket::http::RawStr;
    let path = RawStr::new(&path).percent_decode();

    let path = match path {
        Ok(x) => RawStr::from_cow_str(x).as_str().to_owned(),
        Err(_) => {return Err(Status::BadRequest)},
    };

    let unique_result =
        sqlx::query_as!(Visits, "SELECT * FROM paths WHERE path = $1 LIMIT 1", path)
            .fetch_optional(&mut **db)
            .await;

    match unique_result {
        Ok(x) => match x {
            Some(x) => {
                use rocket::serde::json::Json;
                Ok(Json(x))
            }
            None => Err(Status::NotFound),
        },
        Err(x) => {
            dbg!(x);
            Err(Status::InternalServerError)
        }
    }
}

use std::time::Duration;

trait DurationExt {
    fn from_hours(hours: u64) -> Duration;
    fn from_mins(mins: u64) -> Duration;
    fn from_days(days: u64) -> Duration;
}

impl DurationExt for Duration {
    fn from_hours(hours: u64) -> Duration {
        Duration::from_secs(hours * 60 * 60)
    }
    fn from_mins(mins: u64) -> Duration {
        Duration::from_secs(mins * 60)
    }
    
    fn from_days(days: u64) -> Duration {
        Duration::from_secs(days * 60 * 60 * 24)
    }
}

#[get("/path/id/<id>")]
async fn path_view(mut conn: Connection<Db>, id: u32) -> Result<Template, Status> {
    use std::time::Duration;

    

    let duration = Duration::from_mins(30).as_millis();
    let half_hourly = get_graph(&mut conn, id as i32, duration as i64, "Half Hourly".to_string()).await.unwrap();
    dbg!(&half_hourly);
    
    dbg!("good");

    let duration = Duration::from_days(1).as_millis();
    let daily = get_graph(&mut conn, id as i32, duration as i64, "Daily".to_string()).await.unwrap();

    dbg!("good2");

    let duration = Duration::from_days(30).as_millis();
    let monthly = get_graph(&mut conn, id as i32, duration as i64, "Monthly (30 days)".to_string()).await.unwrap();

    dbg!("good3");

    let x = vec![half_hourly, daily, monthly];
    return Ok(Template::render("analytics/path", context! { graphs: x }));
}

#[get("/<page>")]
async fn analytics_page_view(mut db: Connection<Db>, page: u32) -> Result<Template, Status> {

    let page_limit = 15;
    let ofset = page_limit * (page-1);

    let amount = sqlx::query!(
        "SELECT COUNT(1) as count FROM paths"
    )
    .fetch_one(&mut **db)
    .await
    .unwrap();

    let mut total_pages = amount.count / page_limit as i32;
    let remainder =  amount.count - (total_pages * page_limit as i32);
    if remainder > 0 {
        total_pages += 1;
    }
    
    let routes = sqlx::query_as!(
        Visits,
        "SELECT * FROM paths ORDER BY total_requests DESC LIMIT $1 OFFSET $2",
        page_limit, ofset
    )
    .fetch_all(&mut **db)
    .await;

    match routes {
        Ok(x) => {
            return Ok(Template::render("analytics/index", context! { routes: x, page: page, total_pages: total_pages }));
        },
        Err(_) => Err(Status::InternalServerError),
    }
}

#[get("/")]
fn analytics_index() -> Redirect {
    Redirect::to(uri!("analytics/1"))
}

pub fn routes() -> Vec<rocket::Route> {
    routes![analytics_index, visits_path, visits_id, analytics_page_view, path_view]
}
