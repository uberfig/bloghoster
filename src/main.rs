#[macro_use]
extern crate rocket;

use git2::Repository;
use rocket_analytics::Analytics;

mod pull;

use rocket::{
    fairing::AdHoc,
    fs::{NamedFile, relative, FileServer},
    Request,
};
use std::path::{PathBuf, Path};

fn git_refresh() {
    let url = "https://github.com/uberfig/ivytime.gay.git";

    let repo = match Repository::open("./static") {
        Ok(repo) => repo,
        Err(e) => {
            match Repository::clone(url, "./static") {
                Ok(repo) => repo,
                Err(e) => panic!("failed to clone: {}", e),
            }
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

#[launch]
fn rocket() -> _ {
    git_refresh();
    rocket::build()
    .mount("/", FileServer::from(relative!("static/public")))
    .mount("/refresh", routes![refresh])
    .attach(Analytics::new(include_str!("../secrets/apiKey").to_string()))
    .register("/", catchers![not_found])
}