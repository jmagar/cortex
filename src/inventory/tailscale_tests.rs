use super::*;

#[test]
fn parses_local_tailscale_identity() {
    let mut out = CollectorOutput::new("tailscale");
    parse_status(
        r#"{"Self":{"HostName":"dookie","OS":"linux","TailscaleIPs":["100.64.0.1"]}}"#,
        &mut out,
    );
    assert_eq!(out.nodes[0].hostname, "dookie");
    assert_eq!(out.nodes[0].ips, vec!["100.64.0.1"]);
}
