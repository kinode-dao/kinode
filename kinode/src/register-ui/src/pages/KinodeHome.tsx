import { useEffect } from "react"
import { useNavigate } from "react-router-dom"
import { OPTIMISM_OPT_HEX } from "../constants/chainId";

type OsHomeProps = {
    knsName: string
    nodeChainId: string
}

function KinodeHome({ knsName, nodeChainId }: OsHomeProps) {
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
        document.title = "Welcome | Kinode"
    }, [])

    return (
        <>
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
                            className="alt"
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