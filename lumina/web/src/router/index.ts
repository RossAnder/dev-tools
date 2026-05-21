import { createRouter, createWebHistory } from 'vue-router'
import HierarchyView from '../views/HierarchyView.vue'

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    {
      path: '/',
      name: 'hierarchy',
      component: HierarchyView,
    },
  ],
})

export default router
