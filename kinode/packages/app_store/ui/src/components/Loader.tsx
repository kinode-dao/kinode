import React from 'react'
import { FaCircleNotch } from 'react-icons/fa6'

type LoaderProps = {
  msg: string
}

export default function Loader({ msg }: LoaderProps) {
  return (
    <div id="loading" className="flex-col-center text-center gap-4">
      <h4>{msg}</h4>
      <FaCircleNotch className="animate-spin rounded-full h-8 w-8" />
    </div>
  )
}
