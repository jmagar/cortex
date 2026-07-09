#!/usr/bin/env node
"use strict";
const fs = require("node:fs");
const http = require("node:http");
const https = require("node:https");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { binaryPath, downloadUrl, installRoot, releaseVersion, targetFor } = require("../lib/platform");
function log(message) { process.stderr.write(`cortex-rmcp: ${message}\n`); }
function download(url, destination) { return new Promise((resolve,reject)=>{ const client = url.startsWith("http:") ? http : https; const request=client.get(url,(response)=>{ if([301,302,303,307,308].includes(response.statusCode)){ response.resume(); download(response.headers.location,destination).then(resolve,reject); return; } if(response.statusCode!==200){ response.resume(); reject(new Error(`download failed (${response.statusCode}) from ${url}`)); return; } const file=fs.createWriteStream(destination,{mode:0o600}); response.pipe(file); file.on("finish",()=>file.close(resolve)); file.on("error",reject); }); request.on("error",reject); }); }
function run(command,args) { const result=spawnSync(command,args,{encoding:"utf8"}); if(result.status!==0) throw new Error((result.stderr || result.stdout || `${command} failed`).trim()); }
function extract(archive,destination,target) { fs.rmSync(destination,{recursive:true,force:true}); fs.mkdirSync(destination,{recursive:true}); if(target.archiveType==="zip") { if(process.platform==="win32") run("powershell.exe",["-NoProfile","-Command",`Expand-Archive -LiteralPath '${archive.replace(/'/g,"''")}' -DestinationPath '${destination.replace(/'/g,"''")}' -Force`]); else run("unzip",["-q",archive,"-d",destination]); } else { run("tar",["-xzf",archive,"-C",destination]); } }
async function main() { if(process.env.CORTEX_RMCP_SKIP_DOWNLOAD==="1") { log("skipping binary download because CORTEX_RMCP_SKIP_DOWNLOAD=1"); return; } const target=targetFor(); const destination=binaryPath(); if(fs.existsSync(destination)) { log(`${path.basename(destination)} already installed for ${releaseVersion()}`); return; } const tempDir=fs.mkdtempSync(path.join(os.tmpdir(),"cortex-rmcp-install-")); const archive=path.join(tempDir,target.asset); try { const url=downloadUrl(target); log(`downloading ${url}`); await download(url,archive); extract(archive,installRoot(),target); fs.chmodSync(destination,0o755); log(`installed ${destination}`); } finally { fs.rmSync(tempDir,{recursive:true,force:true}); } }
main().catch((error)=>{ log(error.message); process.exitCode=1; });
