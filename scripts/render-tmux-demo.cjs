#!/usr/bin/env node
// npm install --prefix /tmp/essh-demo-render @xterm/xterm playwright
// NODE_PATH=/tmp/essh-demo-render/node_modules node scripts/render-tmux-demo.cjs
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');
const { execFileSync } = require('child_process');
(async () => {
  const dir = path.resolve(process.env.ESSH_DEMO_OUT || 'artifacts/demo');
  const [header, ...events] = fs.readFileSync(path.join(dir, 'essh-v3-demo.cast'), 'utf8').trim().split('\n').map(JSON.parse);
  const chapters = JSON.parse(fs.readFileSync(path.join(dir, 'chapters.json')));
  const speedups=JSON.parse(fs.readFileSync(path.join(dir,'speedups.json')));
  const editTime=t=>t-speedups.reduce((sum,s)=>sum+Math.max(0,Math.min(t,s.end)-s.start)*(1-s.duration/(s.end-s.start)),0);
  for(const event of events)event[0]=editTime(event[0]);
  for(const chapter of chapters){chapter.time=editTime(chapter.time);if(chapter.sample!==undefined)chapter.sample=editTime(chapter.sample);}
  header.duration=editTime(header.duration);
  fs.writeFileSync(path.join(dir,'edited-chapters.json'),JSON.stringify(chapters,null,2)+'\n');
  const xterm = path.dirname(require.resolve('@xterm/xterm/package.json'));
  // Embed assets so the replay remains usable without npm or a web server.
  const html = `<!doctype html><meta charset="utf-8"><title>ESSH v3 — recorded live demo</title>
<style>${fs.readFileSync(path.join(xterm,'css/xterm.css'),'utf8')}
*{box-sizing:border-box}body{margin:0;background:#080c0f;color:#e0e8ec;font-family:"Noto Sans Mono",monospace}header{padding:20px 30px 16px;height:112px}h1{font-size:23px;margin:0 0 10px}#keys{color:#52d6a0;font-size:17px}#terminal{margin:0 28px;padding:12px;background:#0b1015;border:1px solid #26343e;display:inline-block}footer{padding:12px 30px;font-size:13px;color:#8c9eaa}button{float:right;background:#15232a;color:#e0e8ec;border:1px solid #41606e;padding:8px;cursor:pointer}</style>
<header><button onclick="location.reload()">Replay</button><h1 id="title">ESSH v3 · recorded live demo</h1><div id="keys"></div></header><div id="terminal"></div><footer>Real ESSH release build · isolated loopback SSH fixture · connection waits shortened · actual terminal output <span id="time"></span></footer>
<script>${fs.readFileSync(path.join(xterm,'lib/xterm.js'),'utf8')}</script>
<script>const events=${JSON.stringify(events).replace(/</g,'\\u003c')};const chapters=${JSON.stringify(chapters)};
const term=new Terminal({cols:192,rows:46,fontFamily:'"Noto Sans Mono",monospace',fontSize:14,lineHeight:1.12,scrollback:0,cursorBlink:false,theme:{background:'#0b1015',foreground:'#e0e8ec'},allowProposedApi:true});
term.open(document.getElementById('terminal'));
window.demoReady=true;
window.startDemo=(rate=1)=>{let index=0;const start=performance.now();return new Promise(resolve=>{function frame(){const t=(performance.now()-start)/1000*rate;let data='';while(index<events.length && events[index][0]<=t){data+=events[index++][2];}if(data)term.write(data);const c=chapters.filter(c=>c.time<=t).at(-1);if(c){document.getElementById('title').textContent=c.title;document.getElementById('keys').textContent=c.keys;}document.getElementById('time').textContent=' · '+Math.floor(t)+'s / '+Math.ceil(${header.duration})+'s';if(t<${header.duration})requestAnimationFrame(frame);else{term.write('',()=>resolve());}}frame();});};
if(!location.hash.includes('controlled'))window.startDemo();</script>`;
  const htmlPath=path.join(dir,'replay.html');fs.writeFileSync(htmlPath,html);
  const browser=await chromium.launch({executablePath:process.env.CHROME_PATH || chromium.executablePath(),headless:true,args:['--no-sandbox']});
  const context=await browser.newContext({viewport:{width:1700,height:1200},recordVideo:{dir:path.join(dir,'raw-video'),size:{width:1700,height:1200}},deviceScaleFactor:1});
  const page=await context.newPage();
  page.on('pageerror',error=>{throw error});
  await page.goto('file://'+htmlPath+'#controlled');
  await page.waitForFunction(()=>window.demoReady);
  const bounds=await page.locator('#terminal').boundingBox();
  if(bounds.x+bounds.width>1700 || bounds.y+bounds.height>1200)throw Error('Terminal clipped by video viewport');
  const playback=page.evaluate(()=>window.startDemo(2));
  const snapshot=await browser.newPage({viewport:{width:1700,height:1200}});
  await snapshot.goto('file://'+htmlPath+'#controlled');
  const samples=[];
  for(let i=0;i<chapters.length;i++){
    const end=i+1<chapters.length?chapters[i+1].time:header.duration;
    const when=chapters[i].sample ?? Math.max(chapters[i].time,(chapters[i].time+end)/2);
    await snapshot.evaluate(when=>new Promise(resolve=>{
      term.reset();
      const chapter=chapters.filter(c=>c.time<=when).at(-1);
      document.getElementById('title').textContent=chapter.title;
      document.getElementById('keys').textContent=chapter.keys;
      term.write(events.filter(e=>e[0]<=when).map(e=>e[2]).join(''),resolve);
    }),when);
    const filename=`frame-${String(i+1).padStart(2,'0')}.png`;
    await snapshot.screenshot({path:path.join(dir,filename)});
    const text=await snapshot.evaluate(()=>Array.from({length:term.rows},(_,i)=>term.buffer.active.getLine(i)?.translateToString(true)||'').join('\n'));
    samples.push({chapter:chapters[i].title,time:when,image:filename,text});
    console.log(filename,chapters[i].title);
  }
  await playback;
  fs.writeFileSync(path.join(dir,'rendered-frames.json'),JSON.stringify(samples,null,2)+'\n');
  const required=chapters.flatMap((chapter,index)=>(chapter.expect||[]).map(needle=>[index,needle,chapter.title+': '+needle]));
  for (const [index, needle, label] of required) {
    if(!samples[index].text.includes(needle))throw Error(label+' failed in replay');
  }
  fs.writeFileSync(path.join(dir,'playback-verification.json'),JSON.stringify({
    viewport:{width:1700,height:1200},terminalBounds:bounds,
    checks:required.map(([, , check])=>({check,passed:true}))
  },null,2)+'\n');
  const video=page.video();await context.close();const raw=await video.path();await browser.close();
  fs.writeFileSync(path.join(dir,'rendered-frames.json'),JSON.stringify(samples,null,2)+'\n');
  execFileSync('ffmpeg',['-y','-i',raw,'-vf','setpts=2*PTS','-r','25','-c:v','libx264','-preset','fast','-crf','20','-pix_fmt','yuv420p','-movflags','+faststart',path.join(dir,'essh-v3-demo.mp4')],{stdio:['ignore','ignore','pipe']});
  console.log('Rendered video and standalone replay:',dir);
})().catch(error=>{console.error(error);process.exit(1)});
