import { useState, useEffect } from "react";
import { Navigate, BrowserRouter as Router, Route, Routes, useParams } from 'react-router-dom';

import CommitDotOsName from "./pages/CommitDotOsName";
import MintDotOsName from "./pages/MintDotOsName";
import MintCustom from "./pages/MintCustom";
import SetPassword from "./pages/SetPassword";
import Login from './pages/Login'
import ResetName from './pages/ResetName'
import HyperdriveHome from "./pages/HyperdriveHome"
import ImportKeyfile from "./pages/ImportKeyfile";
import { UnencryptedIdentity } from "./lib/types";
import Header from "./components/Header";
import ProgressBar from "./components/ProgressBar";

function App() {
  const params = useParams()

  const [pw, setPw] = useState<string>('');
  const [key, _setKey] = useState<string>('');
  const [keyFileName, setKeyFileName] = useState<string>('');
  const [reset, setReset] = useState<boolean>(false);
  const [direct, setDirect] = useState<boolean>(false);
  const [hnsName, setHnsName] = useState<string>('');
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

  useEffect(() => {
    (async () => {
      try {
        const infoResponse = await fetch('/info', { method: 'GET', credentials: 'include' })

        if (infoResponse.status > 399) {
          console.log('no info, unbooted')
        } else {
          const info: UnencryptedIdentity = await infoResponse.json()

          if (initialVisit) {
            setHnsName(info.name)
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
    key,
    keyFileName, setKeyFileName,
    reset, setReset,
    pw, setPw,
    hnsName, setHnsName,
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
                : <HyperdriveHome {...props} />
              } />
              <Route path="/commit-os-name" element={
                <>
                  <ProgressBar hnsName={hnsName} />
                  <CommitDotOsName {...props} />
                </>
              } />
              <Route path="/mint-os-name" element={
                <>
                  <ProgressBar hnsName={hnsName} />
                  <MintDotOsName {...props} />
                </>
              } />
              <Route path="/set-password" element={
                <>
                  <ProgressBar hnsName={hnsName} />
                  <SetPassword {...props} />
                </>
              } />
              <Route path="/reset" element={<ResetName {...props} />} />
              <Route path="/import-keyfile" element={<ImportKeyfile {...props} />} />
              <Route path="/login" element={<Login {...props} />} />
              <Route path="/custom-register" element={
                <>
                  <ProgressBar hnsName={hnsName} />
                  <MintCustom {...props} />
                </>
              } />
            </Routes>
          </main>
        </Router>
      </div>
    </>

  )
}

export default App;
