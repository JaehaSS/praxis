use praxis_lib::preview_bridge::validate_preview_probe_url;

#[test]
fn probe_url_allows_only_loopback_http_with_explicit_port() {
    for value in ["http://localhost:1401/probe", "http://127.0.0.1:1402/probe"] {
        assert!(validate_preview_probe_url(value).is_ok(), "{value}");
    }
    for value in [
        "https://localhost:1401/probe",
        "http://example.com:1401/probe",
        "http://localhost/probe",
        "http://user@localhost:1401/probe",
    ] {
        assert!(validate_preview_probe_url(value).is_err(), "{value}");
    }
}
