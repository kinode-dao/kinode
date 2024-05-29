import React from 'react'

type LoaderProps = {
  msg: string
}

export default function Loader({ msg }: LoaderProps) {
  return (
    <div id="loading" className="flex-col-center text-center gap-4">
      <h4>{msg}</h4>
      <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-r-2 border-gray-900"></div>
    </div>
  )
}
