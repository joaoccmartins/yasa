import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [react()],
  // Same-origin in dev too: the session cookie works exactly like on Vercel.
  server: { proxy: { '/api': 'http://127.0.0.1:3000' } },
})
