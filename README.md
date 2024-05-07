this is a simple static site server that can automatically clone and deploy from a git repo and collects basic anonomyzed analytics stored locally

the analytics in this project were motivated by a lack of locally stored analytics for rocket. Currently theres really just [rocket-analytics](https://crates.io/crates/rocket-analytics) which just sends everything to centralized database and the data sent is not anonomyzed

the analytics in this project are intended to respect privacy as much as possible storing ip addresses in a hashed state using sha256 and nothing is shared externally. Due to this, it is unable to provide location data, but if you have cloudflare you should have a decent view of that information