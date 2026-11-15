import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {arg,root,main} from './common.mjs';

main(import.meta.url,()=> {
  const directory=path.resolve(arg('root',root)),web=path.join(directory,'web');
  if(!fs.existsSync(path.join(web,'package.json')))throw new Error('webPackageMissing');
  const result=spawnSync(process.platform==='win32'?'pnpm.cmd':'pnpm',['--dir',web,'run','build'],{cwd:directory,stdio:['ignore','inherit','inherit'],shell:process.platform==='win32'});
  if(result.error||result.status!==0)throw new Error('webBuildFailed');
  if(!fs.existsSync(path.join(directory,'dist/index.html')))throw new Error('webEntryMissing');
  console.log('Web assets built in dist/');
});
