import {test}from'node:test';import assert from'node:assert/strict';import{encodeZip,readZip}from'./archive.mjs';
test('plugin CLI remains executable after packaging without granting setuid',()=>{
 const zip=encodeZip([{name:'tools/gh',data:Buffer.from('official binary fixture'),mode:0o755},{name:'LICENSE',data:Buffer.from('MIT')}]);
 const entries=readZip(zip);assert.equal(entries[0].mode&0o7777,0o755);assert.equal(entries[1].mode&0o7777,0o644);
});
import fs from'node:fs';import path from'node:path';import{root}from'./common.mjs';import{verifyPackage}from'./verify-plugin.mjs';
test('built package retains licensing, verified CLI, private production manifest and no runtime data',{skip:!fs.existsSync(path.join(root,'dist-pack/macos/SHA256SUMS'))},()=>{
 const metadata=verifyPackage(path.join(root,'dist-pack/macos'));assert.equal(metadata.verified,true);
 const file=path.join(root,'dist-pack/macos/otools-releasehub-0.1.0-macos.oplg');const entries=readZip(fs.readFileSync(file));
 const byName=new Map(entries.map(entry=>[entry.name,entry]));assert.equal(byName.get('tools/gh').mode,0o755);
 const manifest=JSON.parse(byName.get('plugin.json').data);assert.equal(manifest.openMode,'panel');assert.equal(manifest.native.enabled,true);assert.equal('devUrl'in manifest,false);assert.equal('quickDev'in manifest,false);
 assert.ok(byName.has('LICENSE'));assert.ok(byName.has('THIRD_PARTY_NOTICES.md'));assert.ok(!entries.some(entry=>/sqlite|token-login|connected\.json|\.releasehub\.lock/.test(entry.name)));
});
