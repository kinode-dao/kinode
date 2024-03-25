import { useEffect } from "react"
import { useNavigate } from "react-router-dom"
import OsHeader from "../components/KnsHeader"
import {ReactComponent as Logo} from "../assets/logo.svg";
import {ReactComponent as NameLogo} from "../assets/kinode.svg"
import { OPTIMISM_OPT_HEX } from "../constants/chainId";

type OsHomeProps = {
    openConnect: () => void
    provider: any
    knsName: string
    closeConnect: () => void
    nodeChainId: string
}

function OsHome({ openConnect, knsName, provider, closeConnect, nodeChainId }: OsHomeProps) {
    const navigate = useNavigate()
    const inviteRedir = () => navigate('/claim-invite')
    const registerEthRedir = () => navigate('/register-eth-name')
    const registerRedir = () => navigate('/register-name')
    const resetRedir = () => navigate('/reset')
    const importKeyfileRedir = () => navigate('/import-keyfile')
    const loginRedir = () => navigate('/login')

    const previouslyBooted = Boolean(knsName)

    const hasNetwork = Boolean(window.ethereum)

    useEffect(() => {
        document.title = "Welcome"
    }, [])

    return (
        <>
            <OsHeader header={<>
                <h3>Welcome to</h3>
                <NameLogo style={{ height: 36 }} />
                <Logo style={{ height: 42, marginTop: 8 }} />
            </>} openConnect={openConnect} closeConnect={closeConnect} hideConnect nodeChainId={nodeChainId} nameLogo />
            <div className="col" style={{ maxWidth: 'calc(100vw - 32px)', width: 460, gap: 20 }}>
                {previouslyBooted ? (
                    <button onClick={loginRedir}> Login </button>
                ) : (
                    <>
                        {!hasNetwork && <h4 style={{ alignSelf: 'flex-start' }}>
                            You must install a Web3 wallet extension like Metamask in order to register or reset a username.
                        </h4>}
                        {hasNetwork && <h4 style={{ alignSelf: 'flex-start' }}>New here? Register a username to get started</h4>}
                        <button disabled={!hasNetwork} onClick={registerRedir} className="alt"> Register Kinode Name </button>
                        <h4 style={{ alignSelf: 'flex-start' }}>Other options</h4>
                        {nodeChainId !== OPTIMISM_OPT_HEX && <button disabled={!hasNetwork} onClick={registerEthRedir} className="alt"> Register ENS Name </button>}
                        <button disabled={!hasNetwork} onClick={inviteRedir} className="alt"> Claim Kinode Invite </button>
                        <button disabled={!hasNetwork} onClick={resetRedir} className="alt"> Reset Kinode Name </button>
                        <button onClick={importKeyfileRedir}> Import Keyfile </button>
                    </>
                )}
            </div>
        </>
    )
}

export default OsHome