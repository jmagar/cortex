use super::*;

#[test]
fn normalizes_remote_device_facts() {
    let mut out = CollectorOutput::new("remote_device");
    normalize_host(
        "dookie",
        "hostname=dookie\nos=Ubuntu\ncpu=Intel\nmemory=16Gi\nip=10.1.0.6\nstorage=ext4\t100\t50\t/\nlistener=tcp LISTEN 0 128 0.0.0.0:3100 0.0.0.0:*\n",
        &mut out,
    );

    assert_eq!(out.nodes[0].hostname, "dookie");
    assert_eq!(out.nodes[0].ips, vec!["10.1.0.6"]);
    assert_eq!(out.nodes[0].listeners[0].port, Some(3100));
    assert_eq!(out.storage[0].mount, "/");
}
