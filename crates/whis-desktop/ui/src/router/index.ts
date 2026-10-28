import { createRouter, createWebHashHistory } from 'vue-router'

const routes = [
  {
    path: '/',
    name: 'home',
    component: () => import('../views/HomeView.vue'),
    meta: { title: 'Whis' },
  },
  {
    path: '/shortcut',
    name: 'shortcut',
    component: () => import('../views/ShortcutView.vue'),
    meta: { title: 'Global Shortcut' },
  },
  {
    path: '/settings',
    name: 'settings',
    component: () => import('../views/SettingsView.vue'),
    meta: { title: 'Settings' },
  },
  {
    path: '/presets',
    name: 'presets',
    component: () => import('../views/PresetsView.vue'),
    meta: { title: 'Presets' },
  },
  {
    path: '/history',
    name: 'history',
    component: () => import('../views/HistoryView.vue'),
    meta: { title: 'History' },
  },
  {
    path: '/about',
    name: 'about',
    component: () => import('../views/AboutView.vue'),
    meta: { title: 'About' },
  },
  {
    path: '/:pathMatch(.*)*',
    name: 'not-found',
    component: () => import('../views/NotFoundView.vue'),
    meta: { title: 'Not Found' },
  },
]

const router = createRouter({
  // Use hash mode for Tauri (no server-side routing)
  history: createWebHashHistory(),
  routes,
})

// Update document title on navigation
router.afterEach((to) => {
  const title = to.meta.title as string | undefined
  document.title = title ? `${title} - Whis` : 'Whis'
})

export default router
