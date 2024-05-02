#[macro_use]
extern crate rocket;

use git2::Repository;
// use rocket_analytics::Analytics;

mod analytics;
mod pull;

use analytics::Db;

use rocket::{
    fairing::{self, AdHoc}, fs::{relative, FileServer, NamedFile}, http::hyper::request, Build, Request, Rocket
};
use rocket_db_pools::Database;
use std::path::{Path, PathBuf};

fn git_refresh() {
    let url = "https://github.com/uberfig/ivytime.gay.git";

    let repo = match Repository::open("./static") {
        Ok(repo) => repo,
        Err(e) => match Repository::clone(url, "./static") {
            Ok(repo) => repo,
            Err(e) => panic!("failed to clone: {}", e),
        },
    };

    //git pull
    let remote_branch = "main";
    let mut remote = repo.find_remote("origin").unwrap();
    let fetch_commit = pull::do_fetch(&repo, &[remote_branch], &mut remote).unwrap();
    let _ = pull::do_merge(&repo, &remote_branch, fetch_commit);
}

#[post("/")]
fn refresh() {
    git_refresh();
}

#[catch(404)]
async fn not_found() -> Option<NamedFile> {
    let path = Path::new(relative!("static/public/404.html"));

    NamedFile::open(path).await.ok()
}

async fn run_migrations(rocket: Rocket<Build>) -> fairing::Result {
    match Db::fetch(&rocket) {
        Some(db) => match sqlx::migrate!().run(&**db).await {
            Ok(_) => Ok(rocket),
            Err(e) => {
                error!("Failed to initialize SQLx database: {}", e);
                Err(rocket)
            }
        },
        None => Err(rocket),
    }
}

pub fn stage() -> AdHoc {
    AdHoc::on_ignite("SQLx Stage", |rocket| async {
        rocket
            .attach(Db::init())
            .attach(AdHoc::try_on_ignite("SQLx Migrations", run_migrations))
        // .mount("/sqlx", routes![list, create, read, delete, destroy])
    })
}

#[launch]
fn rocket() -> _ {
    git_refresh();
    rocket::build()
        .attach(stage())
        .mount("/", FileServer::from(relative!("static/public")))
        .mount("/refresh", routes![refresh])
        // .attach(Analytics::new(include_str!("../secrets/apiKey").to_string()))
        .attach(analytics::Analytics::new())
        .attach(AdHoc::on_response("alalytics", |request, response| {
            Box::pin(async move {
                // do something with the request and pending response...
            })
        }))
        .register("/", catchers![not_found])
}
