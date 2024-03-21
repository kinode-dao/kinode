import { useState, useEffect, useMemo } from "react";
import { Navigate, BrowserRouter as Router, Route, Routes, useParams } from 'react-router-dom';
import { hooks } from "./connectors/metamask";
import {
  KNS_REGISTRY_ADDRESSES,
  DOT_OS_ADDRESSES,
  ENS_REGISTRY_ADDRESSES,
  NAMEWRAPPER_ADDRESSES,
  KNS_ENS_ENTRY_ADDRESSES,
  KNS_ENS_EXIT_ADDRESSES,
} from "./constants/addresses";
import { ChainId } from "./constants/chainId";
import {
  KNSRegistryResolver,
  KNSRegistryResolver__factory,
  DotOsRegistrar,
  DotOsRegistrar__factory,
  KNSEnsEntry,
  KNSEnsEntry__factory,
  KNSEnsExit,
  KNSEnsExit__factory,
  NameWrapper,
  NameWrapper__factory,
  ENSRegistry,
  ENSRegistry__factory
} from "./abis/types";
import { ethers } from "ethers";
import ConnectWallet from "./components/ConnectWallet";
import RegisterEthName from "./pages/RegisterEthName";
import RegisterOsName from "./pages/RegisterKnsName";
import ClaimOsInvite from "./pages/ClaimKnsInvite";
import SetPassword from "./pages/SetPassword";
import Login from './pages/Login'
import Reset from './pages/ResetKnsName'
import OsHome from "./pages/KinodeHome"
import ResetNode from "./pages/ResetNode";
import ImportKeyfile from "./pages/ImportKeyfile";
import { UnencryptedIdentity } from "./lib/types";

const {
  useProvider,
} = hooks;

function App() {
  const provider = useProvider();
  const params = useParams()

  const [pw, setPw] = useState<string>('');
  const [key, setKey] = useState<string>('');
  const [keyFileName, setKeyFileName] = useState<string>('');
  const [reset, setReset] = useState<boolean>(false);
  const [direct, setDirect] = useState<boolean>(false);
  const [knsName, setOsName] = useState<string>('');
  const [appSizeOnLoad, setAppSizeOnLoad] = useState<number>(0);
  const [networkingKey, setNetworkingKey] = useState<string>('');
  const [ipAddress, setIpAddress] = useState<number>(0);
  const [port, setPort] = useState<number>(0);
  const [routers, setRouters] = useState<string[]>([]);
  const [nodeChainId, setNodeChainId] = useState('')

  const [navigateToLogin, setNavigateToLogin] = useState<boolean>(false)
  const [initialVisit, setInitialVisit] = useState<boolean>(!params?.initial)

  const [connectOpen, setConnectOpen] = useState<boolean>(false);
  const openConnect = () => setConnectOpen(true)
  const closeConnect = () => setConnectOpen(false)

  const rpcUrl = useMemo(() => provider?.network?.chainId === ChainId.SEPOLIA ? process.env.REACT_APP_SEPOLIA_RPC_URL : process.env.REACT_APP_OPTIMISM_RPC_URL, [provider])

  const [dotOs, setDotOs] = useState<DotOsRegistrar>(
    DotOsRegistrar__factory.connect(
      provider?.network?.chainId === ChainId.SEPOLIA ? DOT_OS_ADDRESSES[ChainId.SEPOLIA] : DOT_OS_ADDRESSES[ChainId.OPTIMISM],
      new ethers.providers.JsonRpcProvider(rpcUrl))
  );

  const [kns, setKns] = useState<KNSRegistryResolver>(
    KNSRegistryResolver__factory.connect(
      provider?.network?.chainId === ChainId.SEPOLIA ? KNS_REGISTRY_ADDRESSES[ChainId.SEPOLIA] : KNS_REGISTRY_ADDRESSES[ChainId.OPTIMISM],
      new ethers.providers.JsonRpcProvider(rpcUrl))
  );

  const [knsEnsEntry, setKnsEnsEntry] = useState<KNSEnsEntry>(
    KNSEnsEntry__factory.connect(
      provider?.network?.chainId === ChainId.SEPOLIA ? KNS_ENS_ENTRY_ADDRESSES[ChainId.SEPOLIA] : KNS_ENS_ENTRY_ADDRESSES[ChainId.MAINNET],
      // set rpc url based on chain id
      new ethers.providers.JsonRpcProvider(provider?.network?.chainId === ChainId.SEPOLIA ? process.env.REACT_APP_SEPOLIA_RPC_URL : process.env.REACT_APP_MAINNET_RPC_URL))
  );

  const [knsEnsExit, setKnsEnsExit] = useState<KNSEnsExit>(
    KNSEnsExit__factory.connect(
      provider?.network?.chainId === ChainId.SEPOLIA ? KNS_ENS_EXIT_ADDRESSES[ChainId.SEPOLIA] : KNS_ENS_EXIT_ADDRESSES[ChainId.OPTIMISM],
      new ethers.providers.JsonRpcProvider(rpcUrl))
  );

  const [nameWrapper, setNameWrapper] = useState<NameWrapper>(
    NameWrapper__factory.connect(
      provider?.network?.chainId === ChainId.SEPOLIA ? NAMEWRAPPER_ADDRESSES[ChainId.SEPOLIA] : NAMEWRAPPER_ADDRESSES[ChainId.MAINNET],
      new ethers.providers.JsonRpcProvider(rpcUrl))
  );

  const [ensRegistry, setEnsRegistry] = useState<ENSRegistry>(
    ENSRegistry__factory.connect(
      provider?.network?.chainId === ChainId.SEPOLIA ? ENS_REGISTRY_ADDRESSES[ChainId.SEPOLIA] : ENS_REGISTRY_ADDRESSES[ChainId.MAINNET],
      new ethers.providers.JsonRpcProvider(rpcUrl))
  );

  useEffect(() => setAppSizeOnLoad(
    (window.performance.getEntriesByType('navigation') as any)[0].transferSize
  ), []);

  useEffect(() => {
    (async () => {
      try {
        const infoResponse = await fetch('/info', { method: 'GET' })

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
        const currentChainResponse = await fetch('/current-chain', { method: 'GET' })

        if (currentChainResponse.status < 400) {
          const nodeChainId = await currentChainResponse.json()
          setNodeChainId(nodeChainId.toLowerCase())
          console.log('Node Chain ID:', nodeChainId)
        }
      } catch {
        console.log('error getting current chain')
      }
    })()
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => setNavigateToLogin(false), [initialVisit])

  useEffect(() => {
    provider?.getNetwork().then(network => {
      if (network.chainId === ChainId.SEPOLIA) {
        setDotOs(DotOsRegistrar__factory.connect(
          DOT_OS_ADDRESSES[ChainId.SEPOLIA],
          provider!.getSigner()
        ))
        setKns(KNSRegistryResolver__factory.connect(
          KNS_REGISTRY_ADDRESSES[ChainId.SEPOLIA],
          provider!.getSigner()
        ))
        setKnsEnsEntry(KNSEnsEntry__factory.connect(
          KNS_ENS_ENTRY_ADDRESSES[ChainId.SEPOLIA],
          provider!.getSigner()
        ))
        setKnsEnsExit(KNSEnsExit__factory.connect(
          KNS_ENS_EXIT_ADDRESSES[ChainId.SEPOLIA],
          provider!.getSigner()
        ))
        setNameWrapper(NameWrapper__factory.connect(
          NAMEWRAPPER_ADDRESSES[ChainId.SEPOLIA],
          provider!.getSigner()
        ))
        setEnsRegistry(ENSRegistry__factory.connect(
          ENS_REGISTRY_ADDRESSES[ChainId.SEPOLIA],
          provider!.getSigner()
        ))

      } else if (network.chainId === ChainId.OPTIMISM || network.chainId === ChainId.MAINNET) {
        setDotOs(DotOsRegistrar__factory.connect(
          DOT_OS_ADDRESSES[ChainId.OPTIMISM],
          provider!.getSigner())
        )
        setKns(KNSRegistryResolver__factory.connect(
          KNS_REGISTRY_ADDRESSES[ChainId.OPTIMISM],
          provider!.getSigner())
        )
        setKnsEnsExit(KNSEnsExit__factory.connect(
          KNS_ENS_EXIT_ADDRESSES[ChainId.OPTIMISM],
          provider!.getSigner()
        ))
        setKnsEnsEntry(KNSEnsEntry__factory.connect(
          KNS_ENS_ENTRY_ADDRESSES[ChainId.MAINNET],
          provider!.getSigner()
        ))
        setNameWrapper(NameWrapper__factory.connect(
          NAMEWRAPPER_ADDRESSES[ChainId.MAINNET],
          new ethers.providers.JsonRpcProvider(process.env.REACT_APP_MAINNET_RPC_URL)
        ))
        setEnsRegistry(ENSRegistry__factory.connect(
          ENS_REGISTRY_ADDRESSES[ChainId.MAINNET],
          new ethers.providers.JsonRpcProvider(process.env.REACT_APP_MAINNET_RPC_URL)
        ))
      }
    })
  }, [provider])

  const knsEnsEntryNetwork = ChainId.SEPOLIA;
  const knsEnsExitNetwork = ChainId.SEPOLIA;

  // just pass all the props each time since components won't mind extras
  const props = {
    direct, setDirect,
    key,
    keyFileName, setKeyFileName,
    reset, setReset,
    pw, setPw,
    knsName, setOsName,
    dotOs, kns,
    knsEnsEntryNetwork, knsEnsExitNetwork,
    knsEnsEntry, knsEnsExit,
    nameWrapper, ensRegistry,
    connectOpen, openConnect, closeConnect,
    provider, appSizeOnLoad,
    networkingKey, setNetworkingKey,
    ipAddress, setIpAddress,
    port, setPort,
    routers, setRouters,
    nodeChainId,
  }

  return (
    <>
      {
        <>
          <ConnectWallet {...props} />
          <Router>
            <Routes>
              <Route path="/" element={navigateToLogin
                ? <Navigate to="/login" replace />
                : <OsHome {...props} />
              } />
              <Route path="/claim-invite" element={<ClaimOsInvite {...props} />} />
              <Route path="/register-name" element={<RegisterOsName  {...props} />} />
              <Route path="/register-eth-name" element={<RegisterEthName {...props} />} />
              <Route path="/set-password" element={<SetPassword {...props} />} />
              <Route path="/reset" element={<Reset {...props} />} />
              <Route path="/reset-node" element={<ResetNode {...props} />} />
              <Route path="/import-keyfile" element={<ImportKeyfile {...props} />} />
              <Route path="/login" element={<Login {...props} />} />
            </Routes>
          </Router>
        </>
      }
    </>
  )
}

export default App;
