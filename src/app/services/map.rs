use super::*;

fn homelab_map_inventory_sources() -> Vec<HomelabMapInventorySource> {
    vec![
        (
            "devices",
            "~/docs/scripts/devices.sh",
            vec![
                "ssh_hosts",
                "os",
                "cpu",
                "memory",
                "lan_ips",
                "tailscale_ip",
                "storage",
                "gpu",
            ],
        ),
        (
            "docker_containers",
            "~/docs/scripts/docker.sh",
            vec![
                "containers",
                "compose_projects",
                "ports",
                "mounts",
                "networks",
                "env_var_names",
            ],
        ),
        (
            "unraid",
            "~/docs/scripts/unraid.sh",
            vec![
                "array",
                "disks",
                "shares",
                "vms",
                "zfs",
                "replication",
                "plugins",
            ],
        ),
        (
            "nginx_proxies",
            "~/docs/scripts/nginx-proxies.sh",
            vec![
                "swag_configs",
                "endpoints",
                "proxy_targets",
                "authelia",
                "headers",
            ],
        ),
        (
            "unifi",
            "~/docs/scripts/unifi.sh",
            vec![
                "controller",
                "network_devices",
                "clients",
                "wlans",
                "port_forwards",
                "firewall",
            ],
        ),
        (
            "tailscale",
            "~/docs/scripts/tailscale-setup.sh",
            vec!["tailnet_devices", "magicdns", "routes", "exit_nodes", "acl"],
        ),
        (
            "arrs_stack",
            "~/docs/scripts/arrs-stack.sh",
            vec![
                "media_services",
                "download_clients",
                "indexers",
                "plex",
                "tautulli",
            ],
        ),
        (
            "projects",
            "~/docs/scripts/projects.sh",
            vec![
                "git_repos",
                "branches",
                "worktrees",
                "prs",
                "ci_runs",
                "compose_status",
            ],
        ),
        (
            "cortex_db",
            "cortex logs, source inventory, and heartbeat tables",
            vec!["hosts", "source_ips", "apps", "heartbeat_status"],
        ),
    ]
    .into_iter()
    .map(|(name, source, collects)| HomelabMapInventorySource {
        name: name.to_string(),
        source: source.to_string(),
        status: if name == "cortex_db" {
            "included".to_string()
        } else {
            "external_refresh_source".to_string()
        },
        collects: collects.iter().map(|item| item.to_string()).collect(),
    })
    .collect()
}

impl CortexService {
    pub async fn homelab_map(&self, req: HomelabMapRequest) -> ServiceResult<HomelabMapResponse> {
        let host_limit = req.host_limit.unwrap_or(100).clamp(1, 500) as usize;
        let per_host_limit = req.per_host_limit.unwrap_or(10).clamp(1, 25) as usize;

        let (log_hostnames, source_ips_total, apps_total, mut nodes) = self
            .run_db("homelab_map", move |pool| {
                let conn = pool.get()?;
                let mut stmt = conn.prepare(
                    "SELECT hostname, first_seen, last_seen, log_count
                     FROM hosts
                     ORDER BY last_seen DESC",
                )?;
                let mut hosts = stmt
                    .query_map([], |row| {
                        Ok(crate::db::HostEntry {
                            hostname: row.get(0)?,
                            first_seen: row.get(1)?,
                            last_seen: row.get(2)?,
                            log_count: row.get(3)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let log_hostnames = hosts
                    .iter()
                    .map(|host| host.hostname.clone())
                    .collect::<Vec<_>>();
                hosts.truncate(host_limit);

                let source_ips_total = conn.query_row(
                    "SELECT COUNT(DISTINCT source_ip)
                     FROM logs
                     WHERE source_ip IS NOT NULL AND source_ip != ''",
                    [],
                    |row| row.get::<_, i64>(0),
                )? as usize;
                let apps_total = conn.query_row(
                    "SELECT COUNT(DISTINCT app_name)
                     FROM logs
                     WHERE app_name IS NOT NULL AND app_name != ''",
                    [],
                    |row| row.get::<_, i64>(0),
                )? as usize;

                let selected_hosts: HashSet<String> =
                    hosts.iter().map(|host| host.hostname.clone()).collect();
                let mut source_ips_by_host: HashMap<String, Vec<HomelabMapSourceIp>> =
                    HashMap::new();
                let mut stmt = conn.prepare(
                    "SELECT hostname, source_ip, COUNT(*), MIN(received_at), MAX(received_at)
                     FROM logs
                     WHERE source_ip IS NOT NULL AND source_ip != ''
                     GROUP BY hostname, source_ip
                     ORDER BY hostname ASC, COUNT(*) DESC, MAX(received_at) DESC",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        HomelabMapSourceIp {
                            source_ip: row.get(1)?,
                            log_count: row.get(2)?,
                            first_seen: row.get(3)?,
                            last_seen: row.get(4)?,
                        },
                    ))
                })?;
                for row in rows {
                    let (hostname, source_ip) = row?;
                    if !selected_hosts.contains(&hostname) {
                        continue;
                    }
                    let entries = source_ips_by_host.entry(hostname).or_default();
                    if entries.len() < per_host_limit {
                        entries.push(source_ip);
                    }
                }

                let mut apps_by_host: HashMap<String, Vec<HomelabMapApp>> = HashMap::new();
                let mut stmt = conn.prepare(
                    "SELECT hostname, app_name, COUNT(*), MIN(received_at), MAX(received_at)
                     FROM logs
                     WHERE app_name IS NOT NULL AND app_name != ''
                     GROUP BY hostname, app_name
                     ORDER BY hostname ASC, COUNT(*) DESC, MAX(received_at) DESC",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        HomelabMapApp {
                            app_name: row.get(1)?,
                            log_count: row.get(2)?,
                            first_seen: row.get(3)?,
                            last_seen: row.get(4)?,
                        },
                    ))
                })?;
                for row in rows {
                    let (hostname, app) = row?;
                    if !selected_hosts.contains(&hostname) {
                        continue;
                    }
                    let entries = apps_by_host.entry(hostname).or_default();
                    if entries.len() < per_host_limit {
                        entries.push(app);
                    }
                }

                let nodes = hosts
                    .into_iter()
                    .map(|host| HomelabMapNode {
                        hostname: host.hostname.clone(),
                        first_seen: host.first_seen,
                        last_seen: host.last_seen,
                        log_count: host.log_count,
                        source_ips: source_ips_by_host
                            .remove(&host.hostname)
                            .unwrap_or_default(),
                        apps: apps_by_host.remove(&host.hostname).unwrap_or_default(),
                        heartbeat: None,
                    })
                    .collect::<Vec<_>>();

                Ok((log_hostnames, source_ips_total, apps_total, nodes))
            })
            .await?;

        let fleet = self
            .fleet_state(FleetStateRequest {
                include_ok: Some(true),
                sort: Some("hostname".to_string()),
            })
            .await?;
        let heartbeat_hosts = fleet.summary.total;
        let mut all_hostnames: HashSet<String> = log_hostnames.into_iter().collect();
        let mut node_index: HashMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(idx, node)| (node.hostname.clone(), idx))
            .collect();

        for heartbeat in fleet.hosts {
            all_hostnames.insert(heartbeat.hostname.clone());
            if let Some(idx) = node_index.get(&heartbeat.hostname).copied() {
                nodes[idx].heartbeat = Some(heartbeat);
                continue;
            }
            if nodes.len() >= host_limit {
                continue;
            }
            let idx = nodes.len();
            node_index.insert(heartbeat.hostname.clone(), idx);
            nodes.push(HomelabMapNode {
                hostname: heartbeat.hostname.clone(),
                first_seen: heartbeat.last_heartbeat_at.clone(),
                last_seen: heartbeat.last_heartbeat_at.clone(),
                log_count: 0,
                source_ips: Vec::new(),
                apps: Vec::new(),
                heartbeat: Some(heartbeat),
            });
        }

        let total_hosts = all_hostnames.len();
        Ok(HomelabMapResponse {
            schema: "cortex.homelab_map.v1".to_string(),
            generated_at: rfc3339_z(Utc::now()),
            summary: HomelabMapSummary {
                hosts: total_hosts,
                returned_hosts: nodes.len(),
                source_ips: source_ips_total,
                apps: apps_total,
                heartbeat_hosts,
                truncated_hosts: total_hosts > nodes.len(),
            },
            nodes,
            inventory_sources: homelab_map_inventory_sources(),
        })
    }
}
