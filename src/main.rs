#[macro_use]
extern crate rocket;

use git2::Repository;


use rocket::{
    fairing::AdHoc,
    fs::{relative, FileServer},
    Request,
};

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
}

#[post("/")]
fn refresh() {
    git_refresh();
}

#[catch(404)]
fn not_found(req: &Request) -> String {
    format!("I couldn't find '{}'. Try something else?", req.uri())
}

#[launch]
fn rocket() -> _ {
    git_refresh();
    rocket::build()
    .mount("/", FileServer::from(relative!("static/public")))
    .mount("/refresh", routes![refresh])
}