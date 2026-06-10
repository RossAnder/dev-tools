import './assets/main.css'

import { createVaporApp } from 'vue'
import App from './App.vue'

// Pure Vapor mode — no Virtual DOM runtime in the bundle. Every SFC under
// this root must be authored with `<script setup vapor>`; mixing in a
// VDOM-compiled SFC would require `createApp` + `vaporInteropPlugin` instead.
createVaporApp(App).mount('#app')
