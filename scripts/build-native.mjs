import fs from 'node:fs';import path from 'node:path';import {spawnSync}from'node:child_process';
import {root,arg,hostPlatform,variant}from'./common.mjs';import{verifyNative}from'./verify-native.mjs';
const platform=arg('platform',hostPlatform()),arch=arg('arch',process.arch==='arm64'?'arm64':'x64'),selected=variant(platform,arch);
const cross=arch!==(process.arch==='arm64'?'arm64':'x64');const args=['build','--release','--locked','--manifest-path',path.join(root,'native/Cargo.toml'),...(cross?['--target',selected.triple]:[])];
const result=spawnSync('cargo',args,{cwd:root,stdio:'inherit'});if(result.status!==0)throw new Error('Native build failed');
const binary=platform==='macos'?'libotools_releasehub_native.dylib':platform==='windows'?'otools_releasehub_native.dll':'libotools_releasehub_native.so';
const source=path.join(root,'native/target',cross?selected.triple:'','release',binary);verifyNative(source,platform,arch);
fs.mkdirSync(path.join(root,'lib'),{recursive:true});fs.copyFileSync(source,path.join(root,'lib',selected.source));fs.copyFileSync(source,path.join(root,selected.entry));
console.log(`Native ABI verified and staged: ${selected.source}`);
