// Run with NODE_PATH pointing to a runtime containing Playwright.
const { chromium } = require('playwright');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const assert = require('node:assert/strict');
const widget = readFileSync(resolve(__dirname, '../src/views/WidgetIsland.vue'), 'utf8');
const css = [...widget.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map(m => m[1]).join('\n');
const transition = widget.match(/<transition name="lyric-fade"[\s\S]*?<\/transition>/)[0];
(async () => {
  const browser = await chromium.launch({ channel: 'msedge', headless: true });
  try {
    const page = await browser.newPage();
    await page.setContent(`<style>${css}</style><div id="app" style="position:relative;width:180px;height:40px;color:rgb(240,240,240)">${transition}</div>`);
    await page.addScriptTag({ path: require.resolve('vue/dist/vue.global.js') });
    await page.evaluate(() => {
      window.state = Vue.reactive({
        currentLyricLine: { text: 'First cue', startMs: 1000 }, isVideoPlayer: false,
        timedLyricScroll: 0, activeLyricWordIndex: 0, lyricWordProgress: 1,
        plainLyricText: '', currentTrackInfo: 'Track', scrollDist: 0,
        scrollDuration: '4s', scanDuration: '4s', isPlaying: true,
        calculateScroll() {},
      });
      Vue.createApp({ setup: () => window.state }).mount('#app');
    });
    await page.evaluate(() => { window.state.currentLyricLine = { text: 'Second cue', startMs: 2000 }; });
    await page.waitForFunction(() => !!document.querySelector('.lyric-fade-enter-active'), null, { timeout: 1000 });
    await page.waitForFunction(() => document.querySelectorAll('.lyric-render-text').length === 1);
    assert.equal(await page.locator('.lyric-render-text').innerText(), 'Second cue');
    for (const color of ['rgb(240, 240, 240)', 'rgb(20, 20, 20)']) {
      await page.evaluate(color => {
        document.querySelector('#app').style.color = color;
        window.state.currentLyricLine = { text: 'Visible word', startMs: 3000,
          words: [{ text: 'Visible word', startMs: 3000, endMs: 4000 }] };
      }, color);
      for (const progress of [0, 0.5, 1, 0.25]) {
        await page.evaluate(progress => { window.state.lyricWordProgress = progress; }, progress);
        const style = await page.locator('.lyric-word').evaluate(el => ({
          color: getComputedStyle(el).color,
          baseOpacity: getComputedStyle(el, '::before').opacity,
          overlayText: getComputedStyle(el, '::after').content,
          clip: getComputedStyle(el, '::after').clipPath,
          animation: getComputedStyle(el, '::after').animationName,
        }));
        assert.equal(style.color, color);
        assert.equal(style.baseOpacity, '0.35', 'Original dim base must remain visible');
        assert.equal(style.overlayText, '"Visible word"');
        assert.equal(style.clip, 'inset(-6px ' + ((1-progress)*100) + '% -6px 0px)', 'Overlay follows word timing, including backward seek');
        assert.equal(style.animation, 'none', 'Playback clock alone controls the sweep');
      }
    }
    console.log('PASS: timed cue transitions and readable word highlights in light/dark themes.');
    for (const zoom of [1, 1.5, 2]) {
      await page.setContent(`<style>${css} html { zoom: ${zoom}; }</style>
        <div style="position:relative;width:326px;height:50px">
          <div class="island-core-content">
            <div class="inner-wrapper"><div class="music-ctl-box"><div class="music-top-row">
              <div class="album-cover"></div><div class="music-info-mask-box">
                <div class="music-info-text single-line" style="position:relative;width:100%;height:100%">
                  <span class="lyric-render-text"><span class="scroll-inner">Rồi người kia cũng dịu nỗi đau trong lòng</span></span>
                </div>
              </div>
            </div></div></div>
            <div class="audio-spectrum">${'<span class="bar"></span>'.repeat(7)}</div>
          </div>
        </div>`);
      const bounds = await page.evaluate(() => {
        const text = document.querySelector('.scroll-inner');
        const mask = document.querySelector('.music-info-mask-box');
        const width = mask.clientWidth;
        text.style.transform = `translate3d(-${Math.max(0, text.scrollWidth - (width - 12))}px,0,0)`;
        const range = document.createRange();
        range.selectNodeContents(text);
        return { textRight: range.getBoundingClientRect().right, maskRight: mask.getBoundingClientRect().right,
          spectrumLeft: document.querySelector('.audio-spectrum').getBoundingClientRect().left };
      });
      assert.ok(bounds.textRight <= bounds.maskRight - 10 * zoom, `Final text clears fade at zoom ${zoom}`);
      assert.ok(bounds.textRight < bounds.spectrumLeft - 8 * zoom, `Final text clears spectrum at zoom ${zoom}`);
    }
    console.log('PASS: final Vietnamese text clears fade and spectrum at 100%, 150%, and 200% scaling.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
