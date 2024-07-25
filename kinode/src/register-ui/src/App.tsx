import { useState, useEffect } from "react";
import { Navigate, BrowserRouter as Router, Route, Routes, useParams } from 'react-router-dom';

import RegisterKnsName from "./pages/RegisterKnsName";
import SetPassword from "./pages/SetPassword";
import Login from './pages/Login'
import ResetKnsName from './pages/ResetKnsName'
import KinodeHome from "./pages/KinodeHome"
import ImportKeyfile from "./pages/ImportKeyfile";
import { UnencryptedIdentity } from "./lib/types";
import Header from "./components/Header";


function App() {
  const params = useParams()

  const [pw, setPw] = useState<string>('');
  const [key, _setKey] = useState<string>('');
  const [keyFileName, setKeyFileName] = useState<string>('');
  const [reset, setReset] = useState<boolean>(false);
  const [direct, setDirect] = useState<boolean>(false);
  const [knsName, setOsName] = useState<string>('');
  const [appSizeOnLoad, setAppSizeOnLoad] = useState<number>(0);
  const [networkingKey, setNetworkingKey] = useState<string>('');
  const [ipAddress, setIpAddress] = useState<number>(0);
  const [ws_port, setWsPort] = useState<number>(0);
  const [tcp_port, setTcpPort] = useState<number>(0);
  const [routers, setRouters] = useState<string[]>([]);
  const [nodeChainId, setNodeChainId] = useState('')

  const [navigateToLogin, setNavigateToLogin] = useState<boolean>(false)
  const [initialVisit, setInitialVisit] = useState<boolean>(!params?.initial)

  const [connectOpen, setConnectOpen] = useState<boolean>(false);
  const openConnect = () => setConnectOpen(true)
  const closeConnect = () => setConnectOpen(false)


  useEffect(() => setAppSizeOnLoad(
    (window.performance.getEntriesByType('navigation') as any)[0].transferSize
  ), []);

  useEffect(() => {
    (async () => {
      try {
        const infoResponse = await fetch('/info', { method: 'GET', credentials: 'include' })

        if (infoResponse.status > 399) {
          console.log('no info, unbooted')
        } else {
          const info: UnencryptedIdentity = await infoResponse.json()

          if (initialVisit) {
            setOsName(info.name)
            setRouters(info.allowed_routers)
            setNavigateToLogin(true)
            setInitialVisit(false)
          }
        }
      } catch {
        console.log('no info, unbooted')
      }

      try {
        const currentChainResponse = await fetch('/current-chain', { method: 'GET', credentials: 'include' })

        if (currentChainResponse.status < 400) {
          const nodeChainId = await currentChainResponse.json()
          setNodeChainId(nodeChainId.toLowerCase())
          console.log('Node Chain ID:', nodeChainId)
        } else {
          console.error('error processing chain response', currentChainResponse)
        }
      } catch (e) {
        console.error('error getting current chain', e)
      }
    })()
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => setNavigateToLogin(false), [initialVisit])


  // just pass all the props each time since components won't mind extras
  // todo, most of these can be removed...
  const props = {
    direct, setDirect,
    key, appSizeOnLoad,
    keyFileName, setKeyFileName,
    reset, setReset,
    pw, setPw,
    knsName, setOsName,
    connectOpen, openConnect, closeConnect,
    networkingKey, setNetworkingKey,
    ipAddress, setIpAddress,
    ws_port, setWsPort,
    tcp_port, setTcpPort,
    routers, setRouters,
    nodeChainId,
  }

  return (
    <>
      <Header />
      <div id="signup-page" className="container">
        <Router>
          <main>
            <Routes>
              <Route path="/" element={navigateToLogin
                ? <Navigate to="/login" replace />
                : <KinodeHome {...props} />
              } />
              <Route path="/register-name" element={<RegisterKnsName  {...props} />} />
              <Route path="/set-password" element={<SetPassword {...props} />} />
              <Route path="/reset" element={<ResetKnsName {...props} />} />
              <Route path="/import-keyfile" element={<ImportKeyfile {...props} />} />
              <Route path="/login" element={<Login {...props} />} />
            </Routes>
          </main>
        </Router>
      </div>
    </>

  )
}

export default App;
