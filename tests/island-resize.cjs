const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const { runInNewContext } = require('node:vm');
const { test } = require('node:test');
const ts = require('typescript');
const { ref, computed, watch, nextTick, effectScope } = require('vue');
const source = readFileSync(resolve(__dirname, '../src/views/WidgetIsland.vue'), 'utf8');

function setup() {
    const calls = [];
    const scope = effectScope();
    const context = { computed, watch, console, Number, clearTimeout,
        isAnimationLocked: false, isPendingCollapse: false, musicExpandAnimTimer: null,
        animateIslandSize: (w, h) => calls.push([w, h]),
        adjustWindowPosition: async () => {},
        listen: async (_, handler) => { context.settings = handler; },
    };
    for (const [key, value] of Object.entries({
        displayAiQuota: true, isAiQuotaHovered: true,
        enableCodexQuota: true, enableAntigravityQuota: true,
        aiQuotaData: { codex: {}, antigravity: null },
        displayFps: false, displayCustom: false, displayResource: false,
        displaySpeed: false, displayMusic: false, isMsgActive: false,
        displayActivity: false, displaySysToast: false,
        isMusicExpanded: false, isMusicExpanding: false,
        nsdBaseWidth: 150, nsdBaseHeight: 34, nsdMusicBaseWidth: 260,
        nsdMusicExpandedWidth: 320, nsdMsgExpandedWidth: 360,
        nsdBorderRadius: 20, dualAiGlowBoostEnabled: true,
        dynamicIslandTopAttached: false, nsdSpringStyle: 'bouncy',
        nsdLyricDelay: 0, nsdLyricAlignment: 'left', appScale: 1,
        isIslandVisible: true,
    })) context[key] = ref(value);
    const start = source.indexOf('const getBaseSize =');
    const end = source.indexOf('// 专门用于控制右侧', start);
    scope.run(() => runInNewContext(ts.transpile(source.slice(start, end)), context));
    const listenerStart = source.indexOf("    await listen<any>('sync-dynamic-settings'");
    const listenerEnd = source.indexOf('\n    });', listenerStart) + 8;
    runInNewContext(ts.transpile(source.slice(listenerStart, listenerEnd).replace('await listen', 'listen')), context);
    return { context, calls, scope };
}

test('expanded quota fits enabled providers before data arrives and after settings change', async () => {
    const { context: c, calls, scope } = setup();
    try {
        c.isAiQuotaHovered.value = false;
        await nextTick();
        c.isAiQuotaHovered.value = true;
        await nextTick();
        assert.deepEqual(calls.at(-1), [320, 130]);
        c.enableAntigravityQuota.value = false;
        await nextTick();
        assert.deepEqual(calls.at(-1), [320, 90]);
        calls.length = 0;
        c.aiQuotaData.value = { codex: { remaining: 50 }, antigravity: {} };
        await nextTick();
        assert.equal(calls.length, 0, 'data refresh with unchanged dimensions must not restart the spring');
    } finally { scope.stop(); }
});

test('console width changes apply while music or a message is expanded', async () => {
    for (const mode of ['music', 'message']) {
        const { context: c, calls, scope } = setup();
        try {
            c.displayAiQuota.value = false;
            c.displayMusic.value = mode === 'music';
            c.isMusicExpanded.value = mode === 'music';
            c.isMsgActive.value = mode === 'message';
            await nextTick();
            calls.length = 0;
            await c.settings({ payload: {
                baseWidth: 150, baseHeight: 34, musicBaseWidth: 260,
                musicExpandedWidth: 420, msgExpandedWidth: 460, borderRadius: 20,
                dynamicIslandTopAttached: false, springStyle: 'bouncy', appScale: 1,
            } });
            await nextTick();
            assert.deepEqual(calls.at(-1), mode === 'music' ? [420, 135] : [460, 65]);
        } finally { scope.stop(); }
    }
});

test('switching from expanded music to quota releases the music expansion lock', async () => {
    const { context: c, calls, scope } = setup();
    try {
        c.displayAiQuota.value = false;
        c.displayMusic.value = true;
        c.isMusicExpanded.value = true;
        c.isAnimationLocked = true;
        await nextTick();
        c.displayMusic.value = false;
        c.displayAiQuota.value = true;
        await nextTick();
        assert.equal(c.isMusicExpanded.value, false);
        assert.equal(c.isAnimationLocked, false);
        assert.deepEqual(calls.at(-1), [320, 130]);
    } finally { scope.stop(); }
});
