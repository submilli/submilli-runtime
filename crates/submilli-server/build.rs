fn main() {
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rustc-check-cfg=cfg(skip_http_tests)");
    println!("cargo:rerun-if-env-changed=SUBMILLI_SKIP_HTTP_TESTS");
    if std::env::var_os("SUBMILLI_SKIP_HTTP_TESTS").is_some_and(|value| value == "1") {
        println!("cargo:rustc-cfg=skip_http_tests");
    }
}
