// Run with NODE_PATH pointing to a runtime containing Playwright.
const { chromium } = require('playwright');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const { runInNewContext } = require('node:vm');
const ts = require('typescript');
const assert = require('node:assert/strict');
const root = resolve(__dirname, '..');
const widget = readFileSync(resolve(root, 'src/views/WidgetIsland.vue'), 'utf8');
const glow = readFileSync(resolve(root, 'src/components/DuoBoostGlow.vue'), 'utf8');
const styles = text => [...text.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map(m => m[1]).join('\n').replace(/:global\(([^)]+)\)/g, '$1');
function computedStyle(name, attached, radius, expanded) {
  const start = widget.indexOf(`const ${name} = computed`);
  const end = widget.indexOf('\n});', start) + 4;
  const context = { computed: f => f(), islandOpacity: {value:100}, islandTheme: {value:'black'}, showCoverglassBg: {value:false}, dynamicIslandTopAttached: {value:attached}, isExpandedSize: {value:expanded}, nsdBorderRadius: {value:radius}, GLOW_SIDE:24, GLOW_BOTTOM:24, GLOW_TOP:12 };
  const js = ts.transpile(widget.slice(start, end) + `\nresult = ${name};`);
  runInNewContext(js, context);
  return context.result;
}
(async () => {
  const browser = await chromium.launch({channel:'msedge', headless:true});
  try {
    const page = await browser.newPage({viewport:{width:374,height:86}});
    await page.emulateMedia({reducedMotion:'reduce'});
    const wrapper = widget.includes('class="island-viewport"');
    for (const zoom of [1, 1.5]) for (const attached of [false,true]) for (const boost of [false,true]) for (const locked of [false,true]) for (const radius of [12,100]) {
      const w=326,h=50;
      await page.goto('about:blank');
      await page.setViewportSize({width:Math.round((w+48)*zoom),height:Math.round((h+36)*zoom)});
      const ring = boost ? `<div class="duo-boost-wrapper"><div class="duo-outer-glow"></div><div class="duo-neon-rim ${attached?'top-attached':''}"></div></div>` : '<div class="rainbow-border-glow"></div>';
      await page.setContent(`<style>${styles(widget)}${styles(glow)} html{zoom:${zoom}}</style><div id="app">${wrapper ? '<div class="island-viewport">':''}<div class="island-container ${attached?'top-attached':''} ${boost?'dual-ai-glow-boost':''} ${locked&&!attached?'locked-top-detached':''}">${ring}<div class="island-core-content">CDX 100% · AGY 96%</div></div>${wrapper?'</div>':''}</div>`);
      const outer=computedStyle('islandStyle',attached,radius,false), inner=computedStyle('coreContentStyle',attached,radius,false);
      await page.evaluate(({outer,inner,attached})=>{
        const host=document.querySelector('.island-viewport');
        if(host) host.style.setProperty('--island-top-gap',attached?'0px':'12px');
        Object.assign(document.querySelector('.island-container').style,outer);
        Object.assign(document.querySelector('.island-core-content').style,inner);
      },{outer,inner,attached});
      await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(resolve)));
      const rect=await page.evaluate(()=>{
        const pill=document.querySelector('.island-container'), core=document.querySelector('.island-core-content');
        const a=pill.getBoundingClientRect(),b=core.getBoundingClientRect(),s=getComputedStyle(core), ring=getComputedStyle(document.querySelector('.duo-neon-rim,.rainbow-border-glow'));
        return {top:a.top,height:a.height,innerHeight:b.height,innerTop:b.top-a.top,topRadius:s.borderTopLeftRadius,bottomRadius:s.borderBottomLeftRadius,outerBottomRadius:getComputedStyle(pill).borderBottomLeftRadius,mask:ring.maskImage,ringTop:ring.paddingTop};
      });
      const label=JSON.stringify({zoom,attached,boost,locked,radius,rect});
      assert.ok(Math.abs(rect.top-(attached?0:12)*zoom)<1,label);
      assert.ok(Math.abs(rect.height-h*zoom)<1,label);
      assert.ok(Math.abs(rect.innerHeight-(h-(attached?2:4))*zoom)<1,label);
      assert.ok(Math.abs(rect.innerTop-(attached?0:2)*zoom)<1,label);
      assert.equal(rect.topRadius,attached?'0px':`${Math.max(radius-2,8)}px`,label);
      assert.equal(rect.outerBottomRadius,`${attached?Math.min(radius,20):radius}px`,label);
      assert.equal(rect.bottomRadius,`${Math.max((attached?Math.min(radius,20):radius)-2,8)}px`,label);
      assert.notEqual(rect.mask,'none',label);
      assert.equal(rect.ringTop,attached?'0px':'2px',label);
    }
    console.log('PASS: 32 layout combinations (lock, attachment, normal/Duo, radius, scaling).');
    await page.emulateMedia({reducedMotion:'no-preference'});
    for(const attached of [false,true,false]) {
      await page.evaluate(({outer,inner,attached})=>{
        document.querySelector('.island-viewport').style.setProperty('--island-top-gap',attached?'0px':'12px');
        const pill=document.querySelector('.island-container');
        pill.classList.toggle('top-attached',attached);
        document.querySelector('.duo-neon-rim').classList.toggle('top-attached',attached);
        Object.assign(pill.style,outer);
        Object.assign(document.querySelector('.island-core-content').style,inner);
      },{outer:computedStyle('islandStyle',attached,100,false),inner:computedStyle('coreContentStyle',attached,100,false),attached});
      await page.waitForFunction(attached=>{
        const pill=document.querySelector('.island-container').getBoundingClientRect();
        return Math.abs(pill.top-(attached?0:18))<.1 && Math.abs(pill.height-75)<.1;
      },attached,{timeout:3000});
      await page.waitForTimeout(350);
      assert.ok(Math.abs(await page.locator('.island-container').evaluate(el=>el.getBoundingClientRect().top)-(attached?0:18))<.1,'No snap back after transition');
      assert.equal(await page.locator('.island-core-content').evaluate(el=>getComputedStyle(el).borderTopLeftRadius),attached?'0px':'98px','Content corners finish at the selected shape');
    }
    if(process.env.ISLAND_QA_IMAGE) await page.screenshot({path:process.env.ISLAND_QA_IMAGE});
    console.log('PASS: attach/detach transition in both directions, stable height and final position.');
  } finally { await browser.close(); }
})().catch(e=>{console.error(e);process.exitCode=1;});
