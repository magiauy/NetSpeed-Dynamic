import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
    history: createWebHistory(),
    routes: [
        { path: '/', component: () => import('../views/MainPanel.vue') },
        { path: '/widget', component: () => import('../views/WidgetIsland.vue') }
    ]
})

export default router
