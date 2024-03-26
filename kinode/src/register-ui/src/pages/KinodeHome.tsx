import { useEffect } from "react"
import { useNavigate } from "react-router-dom"
import KinodeHeader from "../components/KnsHeader"
import { ReactComponent as Logo } from "../assets/logo.svg";
import { ReactComponent as NameLogo } from "../assets/kinode.svg"
import { OPTIMISM_OPT_HEX } from "../constants/chainId";

type OsHomeProps = {
    openConnect: () => void
    provider: any
    knsName: string
    closeConnect: () => void
    nodeChainId: string
}

function KinodeHome({ openConnect, knsName, provider, closeConnect, nodeChainId }: OsHomeProps) {
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
            <KinodeHeader header={<div className="flex flex-col place-items-center self-stretch mt-8">
                <h1>Welcome to</h1>
                <h1 className="display mt-8 text-6xl relative">
                    Kinode
                    <span className="text-[8px] absolute bottom-2">&reg;</span>
                </h1>
            </div>} openConnect={openConnect} closeConnect={closeConnect} hideConnect nodeChainId={nodeChainId} nameLogo />
            <div className="flex flex-col max-w-[460px] w-full gap-4 mt-8">
                {previouslyBooted ? (
                    <button onClick={loginRedir}> Login </button>
                ) : (
                    <>
                        {!hasNetwork && <h4 className="self-start mx-auto">
                            You must install a Web3 wallet extension like Metamask in order to register or reset a username.
                        </h4>}
                        {hasNetwork && <h4 className="self-start mx-auto">
                            New here? Register a username to get started
                        </h4>}
                        <button
                            disabled={!hasNetwork}
                            onClick={registerRedir}
                            className="alt"
                        >
                            Register Kinode Name
                        </button>
                        <h4 className="self-start mx-auto">
                            Other options
                        </h4>
                        {nodeChainId !== OPTIMISM_OPT_HEX && <button
                            disabled={!hasNetwork}
                            onClick={registerEthRedir}
                            className="alt"
                        >
                            Register ENS Name
                        </button>}
                        <button
                            disabled={!hasNetwork}
                            onClick={inviteRedir}
                            className="alt"
                        >
                            Claim Kinode Invite
                        </button>
                        <button
                            disabled={!hasNetwork}
                            onClick={resetRedir}
                            className="alt"
                        >
                            Reset Kinode Name
                        </button>
                        <button
                            onClick={importKeyfileRedir}
                        >
                            Import Keyfile
                        </button>
                    </>
                )}
            </div>
        </>
    )
}

export default KinodeHome