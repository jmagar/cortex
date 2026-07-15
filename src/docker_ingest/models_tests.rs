use super::*;

fn meta(compose_project: Option<&str>, compose_service: Option<&str>, name: &str) -> ContainerMeta {
    ContainerMeta {
        id: "abcdef1234567890".into(),
        name: name.into(),
        image: "nginx:latest".into(),
        compose_project: compose_project.map(str::to_string),
        compose_service: compose_service.map(str::to_string),
    }
}

#[test]
fn app_name_is_flat_compose_service_when_present() {
    let m = meta(Some("edge"), Some("nginx"), "nginx-1");
    assert_eq!(m.app_name(), "nginx");
}

#[test]
fn app_name_uses_compose_service_even_without_project() {
    let m = meta(None, Some("nginx"), "nginx-1");
    assert_eq!(m.app_name(), "nginx");
}

#[test]
fn app_name_falls_back_to_container_name_without_service_label() {
    let m = meta(None, None, "nginx-1");
    assert_eq!(m.app_name(), "nginx-1");
}

#[test]
fn app_name_never_contains_a_slash() {
    let with_service = meta(Some("edge"), Some("nginx"), "nginx-1");
    let without_service = meta(None, None, "nginx-1");
    assert!(!with_service.app_name().contains('/'));
    assert!(!without_service.app_name().contains('/'));
}
