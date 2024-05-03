#[macro_use]
extern crate rocket;

use git2::Repository;
// use rocket_analytics::Analytics;

mod analytics;
mod pull;

use analytics::Db;

use rocket::{
    // fairing::{self, AdHoc}, fs::{relative, FileServer, NamedFile}, http::hyper::request, Build, Request, Rocket
    fairing::{self, AdHoc},
    fs::{relative, FileServer, NamedFile},
    Build,
    Rocket,
};
use rocket_db_pools::Database;
use rocket_dyn_templates::Template;
// use std::path::{Path, PathBuf};
use std::path::Path;

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
    let x = pull::do_merge(&repo, &remote_branch, fetch_commit);
    dbg!(x);
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
    })
}

#[launch]
fn rocket() -> _ {
    git_refresh();
    rocket::build()
        .attach(Template::fairing())
        .attach(stage())
        .mount("/", FileServer::from(relative!("static/public")))
        .mount("/analytics", analytics::routes())
        .mount("/refresh", routes![refresh])
        .attach(analytics::Analytics::new())
        .register("/", catchers![not_found])
}
