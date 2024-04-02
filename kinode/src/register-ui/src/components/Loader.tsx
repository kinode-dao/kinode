type LoaderProps = {
  msg: string
}

export default function Loader({ msg }: LoaderProps) {
  return (
    <div id="loading" className="flex flex-col text-center">
      <h3>{msg}</h3>
      <div id="loader"> <div /> <div /> <div /> <div /> </div>
    </div>
  )
}