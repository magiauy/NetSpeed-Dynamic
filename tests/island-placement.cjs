const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const {runInNewContext} = require('node:vm');
const ts = require('typescript');
const source = readFileSync(require('node:path').resolve(__dirname,'../src/views/WidgetIsland.vue'),'utf8');
const start=source.indexOf('const adjustWindowPosition =');
const end=source.indexOf('\nconst onEnter =',start);
async function check(expanded, scale, monitorY) {
  const calls=[]; let time=0, size={width:374,height:86};
  const win={setSize:async s=>{size=s;calls.push(['size',time,s]);},innerSize:async()=>size,setPosition:async p=>calls.push(['position',time,p])};
  const context={console,Promise,placementQueue:Promise.resolve(),placementRevision:0,latestAnimationRequest:0,nativeResizeUntil:500,performance:{now:()=>time},setTimeout:fn=>{time=500;fn();},getCurrentWindow:()=>win,currentMonitor:async()=>({position:{x:-1920,y:monitorY},size:{width:1920,height:1080},scaleFactor:scale}),availableMonitors:async()=>[],getBaseSize:()=>({w:326,h:50}),GLOW_SIDE:24,GLOW_TOP:12,GLOW_BOTTOM:24,appScale:{value:1.25},isMusicExpanded:{value:expanded},nsdMusicExpandedWidth:{value:400},isMsgActive:{value:false},nsdMsgExpandedWidth:{value:360},displayActivity:{value:false},islandHiddenCenter:null,PhysicalSize:class{constructor(width,height){this.width=width;this.height=height;}},PhysicalPosition:class{constructor(x,y){this.x=x;this.y=y;}}};
  runInNewContext(ts.transpile(source.slice(start,end)+'\nrun = adjustWindowPosition;'),context);
  // Rapid requests supersede one another before touching native geometry.
  await Promise.all([context.run(),context.run(),context.run()]);
  assert.equal(calls.length,2);
  assert.equal(calls[0][1],500,'Must wait for the native resize spring');
  assert.equal(calls[0][2].height,Math.round(((expanded?135:50)+36)*1.25*scale));
  assert.equal(calls[1][2].y,monitorY,'Anchor to selected monitor, not desktop origin or +12');
  assert.equal(calls[1][2].x,Math.round(-1920+(1920-size.width)/2));
}
(async()=>{for(const expanded of [false,true]) for(const scale of [1,1.5,2]) for(const y of [0,-1080]) await check(expanded,scale,y);console.log('PASS: 12 native placement scenarios, queued requests, animation ordering, expanded sizes and monitor DPI.');})().catch(e=>{console.error(e);process.exitCode=1;});
