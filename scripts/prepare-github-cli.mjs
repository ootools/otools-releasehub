import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {root,arg,hostPlatform,variant,main} from './common.mjs';
export const CLI_VERSION='2.100.0';
export const CLI_ASSETS={
  'macos-arm64':['macOS_arm64.zip','45f9a62da2f6e641a7fad57e2ce39656dfd7ef331372d80a2a2aed65abb01642'],
  'macos-x64':['macOS_amd64.zip','fcd7799e85eb575f3c7d2b1679bfbfedaefa1269d4bc7d096b51e10939b4812b'],
  'windows-x64':['windows_amd64.zip','227e35230b25db3fa1b997bab7cf4d67df0470a3b75b99e4ee66bce1a7cd4e72'],
  'windows-arm64':['windows_arm64.zip','7beaeb4743cf255809a8e574a2724c685b566545e04eabea284fe38a56c15b02'],
  'linux-x64':['linux_amd64.tar.gz','e4d4bb4498e8d007abe545b6568926793ace1b6447da598294a610018cb164be'],
  'linux-arm64':['linux_arm64.tar.gz','ea4e7a581a32ccad6cc7923cb1576ac5859ba4b9a16ab22eb8f8a96e78e2e961'],
};
export async function prepareCli(directory,platform,arch){
  const selected=variant(platform,arch),[asset,digest]=CLI_ASSETS[`${platform}-${arch}`];
  const filename=`gh_${CLI_VERSION}_${asset}`,cache=path.join(directory,'.cache/github-cli');fs.mkdirSync(cache,{recursive:true});
  const archive=path.join(cache,filename);
  if(!fs.existsSync(archive)){
    const result=spawnSync('curl',['--fail','--location','--retry','3','--output',archive,`https://github.com/cli/cli/releases/download/v${CLI_VERSION}/${filename}`],{stdio:'inherit'});
    if(result.status!==0)throw new Error('Official GitHub CLI download failed');
  }
  if(createHash('sha256').update(fs.readFileSync(archive)).digest('hex')!==digest)throw new Error('Official GitHub CLI checksum mismatch');
  const stage=path.join(cache,`extracted-${platform}-${arch}`);fs.rmSync(stage,{recursive:true,force:true});fs.mkdirSync(stage,{recursive:true});
  const result=spawnSync('tar',['-xf',archive,'-C',stage],{stdio:'inherit'});if(result.status!==0)throw new Error('CLI archive extraction failed');
  const files=[];function walk(dir){for(const name of fs.readdirSync(dir)){const file=path.join(dir,name);if(fs.statSync(file).isDirectory())walk(file);else files.push(file);}}walk(stage);
  const binary=files.find(file=>path.basename(file)===(platform==='windows'?'gh.exe':'gh')&&path.basename(path.dirname(file))==='bin');
  const license=files.find(file=>path.basename(file)==='LICENSE');if(!binary||!license)throw new Error('CLI archive entries missing');
  const tools=path.join(directory,'tools',selected.suffix);fs.mkdirSync(tools,{recursive:true});fs.copyFileSync(binary,path.join(tools,platform==='windows'?'gh.exe':'gh'));fs.copyFileSync(license,path.join(tools,'LICENSE.github-cli.txt'));
  if(platform!=='windows')fs.chmodSync(path.join(tools,'gh'),0o755);
  fs.writeFileSync(path.join(tools,'github-cli-version.txt'),`${CLI_VERSION}\n`);
  const hashes=fs.readdirSync(tools).filter(name=>name!=='SHA256SUMS').map(name=>`${createHash('sha256').update(fs.readFileSync(path.join(tools,name))).digest('hex')}  ${name}`).join('\n');fs.writeFileSync(path.join(tools,'SHA256SUMS'),hashes+'\n');
  console.log(`Prepared official GitHub CLI ${CLI_VERSION} for ${selected.suffix}; no account data copied.`);return tools;
}
main(import.meta.url,()=>prepareCli(root,arg('platform',hostPlatform()),arg('arch',process.arch==='arm64'?'arm64':'x64')));
