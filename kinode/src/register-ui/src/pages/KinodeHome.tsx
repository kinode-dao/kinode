import { useEffect } from "react"
import { useNavigate } from "react-router-dom"

type OsHomeProps = {
    knsName: string
    nodeChainId: string
}

function KinodeHome({ knsName }: OsHomeProps) {
    const navigate = useNavigate()
    const registerRedir = () => navigate('/register-name')
    const resetRedir = () => navigate('/reset')
    const importKeyfileRedir = () => navigate('/import-keyfile')
    const loginRedir = () => navigate('/login')

    const previouslyBooted = Boolean(knsName)

    useEffect(() => {
        document.title = "Welcome | Kinode"
    }, [])

    return (
        <>
            <div className="container fade-in">
                <div className="section">
                    <div className="content">
                        {previouslyBooted ? (
                            <div className="text-center">
                                <h2 className="mb-2">Welcome back!</h2>
                                <button onClick={loginRedir} className="button">Login</button>
                            </div>
                        ) : (
                            <>
                                <h2 className="text-center mb-2">Welcome to Kinode</h2>
                                <h4 className="text-center mb-2">New here? Register a username to get started</h4>
                                <div className="button-group">
                                    <button onClick={registerRedir} className="button">
                                        Register Kinode Name
                                    </button>
                                </div>
                                <h4 className="text-center mt-2 mb-2">Other options</h4>
                                <div className="button-group">
                                    <button onClick={resetRedir} className="button secondary">
                                        Reset Kinode Name
                                    </button>
                                    <button onClick={importKeyfileRedir} className="button secondary">
                                        Import Keyfile
                                    </button>
                                </div>
                            </>
                        )}
                    </div>
                </div>
            </div>
        </>
    )
}

export default KinodeHome