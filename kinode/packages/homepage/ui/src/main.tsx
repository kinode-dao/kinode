import React from 'react'
import ReactDOM from 'react-dom/client'
import Homepage from './pages/Homepage.tsx'
import { BrowserRouter, Route, Routes } from 'react-router-dom'
import '@unocss/reset/tailwind.css'
import 'uno.css'
import './index.css'
import { Settings } from './pages/Settings.tsx'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <BrowserRouter>
      <Routes>
        <Route path='/' element={<Homepage />} />
        <Route path='/settings' element={<Settings />} />
      </Routes>
    </BrowserRouter>
  </React.StrictMode>,
)
