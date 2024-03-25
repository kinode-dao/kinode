type LoaderProps = {
  msg: string
}

export default function Loader({ msg } : LoaderProps) {
  return (
    <div id="loading" className="col">
      <h3>{msg}</h3>
      <div id="loader"> <div/> <div/> <div/> <div/> </div>
    </div>
  )
}