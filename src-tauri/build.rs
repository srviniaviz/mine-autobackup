fn main() {
    let _ = dotenvy::from_path("../.env");
    println!("cargo:rerun-if-changed=../.env");
    if let Ok(client_id) = std::env::var("GOOGLE_OAUTH_CLIENT_ID") {
        println!("cargo:rustc-env=GOOGLE_OAUTH_CLIENT_ID={client_id}");
    }
    if let Ok(client_secret) = std::env::var("GOOGLE_OAUTH_CLIENT_SECRET") {
        println!("cargo:rustc-env=GOOGLE_OAUTH_CLIENT_SECRET={client_secret}");
    }
    tauri_build::build()
}
