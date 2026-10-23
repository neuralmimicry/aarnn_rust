/* Actual authenticated native physics viewer; no mocked neural or physics path. */
const fs=require('node:fs'),path=require('node:path'),os=require('node:os'),cp=require('node:child_process'),readline=require('node:readline'),assert=require('node:assert/strict');
let playwright;
try {playwright=require('playwright');} catch {
 const cache=path.join(os.homedir(),'.npm','_npx');
 for(const entry of fs.existsSync(cache)?fs.readdirSync(cache):[]) {
  const modulePath=path.join(cache,entry,'node_modules','playwright');
  if(fs.existsSync(path.join(modulePath,'package.json'))&&JSON.parse(fs.readFileSync(path.join(modulePath,'package.json'),'utf8')).version==='1.60.0'){playwright=require(modulePath);break;}
 }
 if(!playwright)throw Error('Playwright unavailable: install the QA-only playwright@1.60.0 or set NODE_PATH');
}
const {chromium}=playwright;
(async()=>{
 const output=path.resolve(process.argv[2]),temp=fs.mkdtempSync(path.join(os.tmpdir(),'aarnn-habitat-browser-'));fs.mkdirSync(output,{recursive:true});
 const token=path.join(temp,'token'),child=cp.spawn(path.resolve('target/debug/aarnn-physics-service'),['--profile','celegans','--control','silent','--port','0','--token-file',token],{stdio:['ignore','pipe','pipe']});
 let browser;const errors=[];child.stderr.on('data',b=>fs.appendFileSync(path.join(output,'browser-service.log'),b));
 try {
  const ready=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('native service readiness')),15000),lines=readline.createInterface({input:child.stdout});lines.once('line',line=>{clearTimeout(timer);resolve(JSON.parse(line));});child.once('exit',()=>{clearTimeout(timer);reject(Error('native service exited'));});});
  browser=await chromium.launch({headless:true,args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
  const page=await browser.newPage({viewport:{width:1440,height:1000}});page.on('pageerror',e=>errors.push(e.message));
  const requests=[];page.on('request',r=>requests.push(new URL(r.url()).pathname));
  await page.goto(ready.url);await page.fill('#token',fs.readFileSync(token,'utf8').trim());await page.click('#connect');
  await page.waitForFunction(()=>{try{return JSON.parse(document.querySelector('#status').textContent).habitat==='Agar microhabitat';}catch{return false;}});
  await page.locator('#view').screenshot({path:path.join(output,'worm-webgl.png')});
  assert.deepEqual(errors,[]);assert(!requests.some(p=>p.includes('/aer')));
  const status=JSON.parse(await page.locator('#status').textContent());assert.equal(status.authority,'MuJoCo-3.17.0-Rust-reference');
  await page.click('#detach');assert.equal(child.exitCode,null);
  fs.writeFileSync(path.join(output,'browser.json'),JSON.stringify({status:'pass',scope:'actual native service browser rendering and independent detach; no neural/task claim',view:status,requests},null,2));
 }finally {if(browser)await browser.close();child.kill('SIGTERM');await new Promise(resolve=>{if(child.exitCode!==null)resolve();else child.once('exit',resolve);});fs.rmSync(temp,{recursive:true,force:true});}
})().catch(e=>{console.error(e);process.exitCode=1;});
