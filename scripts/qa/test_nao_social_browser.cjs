/* Real browser and real local Rust inference. Only microphone/TTS devices are controlled. */
const {chromium}=require('playwright');
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
(async()=>{
 const settings=JSON.parse(fs.readFileSync(process.env.NM_NAO_QA_SESSION)),out=process.env.NM_NAO_QA_OUTPUT;
 const browser=await chromium.launch({executablePath:process.env.NM_CHROMIUM||undefined,headless:true,args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
 try{
  const context=await browser.newContext({viewport:{width:1440,height:1050}});
  await context.addInitScript(()=>{
   window.voiceTest={starts:0,aborts:0,spoken:[]};
   window.SpeechRecognition=class {start(){window.voiceTest.starts++;window.latestRecognition=this;}abort(){window.voiceTest.aborts++;}};
   Object.defineProperty(window,'speechSynthesis',{value:{cancel(){},speak(u){window.voiceTest.spoken.push(u.text);}}});
   window.SpeechSynthesisUtterance=class{constructor(text){this.text=text;}};
  });
  const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.goto(settings.url+'/world?robot=zebrafish&nao_social=1');
  assert.equal(await page.inputValue('#webgl-robot'),'nao');
  await page.fill('#nao-body-token',settings.adapter_token);await page.click('#webgl-connect');
  await page.waitForFunction(()=>Number(document.querySelector('#webgl-step').textContent)>2);
  const frame=page.frameLocator('#nao-chat-window');
  await frame.locator('#invite').fill(settings.join_token);await frame.locator('#connect').click();
  await frame.locator('#history .nao').filter({hasText:'What can you teach me'}).waitFor({timeout:30000});
  await frame.locator('#message').fill('hello');await frame.locator('#send').click();
  await frame.locator('#history .nao').filter({hasText:'Hello! I am NAO.'}).waitFor({timeout:30000});
  await page.locator('#nao-bubble').filter({hasText:'Hello! I am NAO.'}).waitFor();
  await frame.locator('#voice-consent').check();await frame.locator('#speak').check();await frame.locator('#mic').click();
  const chat=page.frames().find(f=>f.url()===settings.url+'/');
  assert.equal(await chat.evaluate(()=>window.voiceTest.starts),1);
  await chat.evaluate(()=>window.latestRecognition.onresult({results:[[{transcript:'name',confidence:.9}]]}));
  assert.equal(await frame.locator('#message').inputValue(),'name');
  assert.equal(await frame.locator('#history .nao').count(),2); // Dictation never auto-sends.
  await frame.locator('#send').click();
  await frame.locator('#history .nao').filter({hasText:'simulated robot connected to AARNN'}).waitFor({timeout:30000});
  assert.equal((await chat.evaluate(()=>window.voiceTest.spoken)).length,1);
  await page.screenshot({path:path.join(out,'nao-conversation.png')});
  await frame.locator('#stop').click();assert.equal(await frame.locator('#message').isDisabled(),true);
  assert.equal(await frame.locator('#voice-consent').isChecked(),false);
  await frame.locator('#invite').fill(settings.join_token);await frame.locator('#connect').click();
  await frame.locator('#history .nao').filter({hasText:'What can you teach me'}).nth(1).waitFor({timeout:30000});
  await page.click('#webgl-connect');
  await page.setViewportSize({width:390,height:844});await page.screenshot({path:path.join(out,'nao-mobile.png')});
  assert.deepEqual(errors,[]);console.log('PASS: neural encounter/reply, bubble, focused text, editable transcript, separate TTS, stop/rejoin and mobile render');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
