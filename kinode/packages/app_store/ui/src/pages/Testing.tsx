import React, { useState, useEffect } from 'react'
import useAppsStore from '../store'

const Testing: React.FC = () => {
    const {
        fetchListings,
        fetchInstalled,
        fetchDownloads,
        fetchOurApps,
        fetchDownloadsForApp,
        listings,
        installed,
        downloads,
        ourApps
    } = useAppsStore()
    const [result, setResult] = useState<any>(null)
    const [appId, setAppId] = useState('')

    useEffect(() => {
        fetchListings()
        fetchInstalled()
        fetchDownloads()
        fetchOurApps()
    }, [])

    const handleAction = async (action: () => Promise<void>, key: string) => {
        try {
            await action()
            setResult(JSON.stringify(useAppsStore.getState()[key], null, 2))
        } catch (error) {
            setResult(`Error: ${error.message}`)
        }
    }

    const handleDownloadsForApp = async () => {
        try {
            const data = await fetchDownloadsForApp(appId)
            setResult(JSON.stringify(data, null, 2))
        } catch (error) {
            setResult(`Error: ${error.message}`)
        }
    }

    return (
        <div>
            <h1>Testing Page</h1>
            <div>
                <button onClick={() => handleAction(fetchListings, 'listings')}>Refresh Listings</button>
                <button onClick={() => handleAction(fetchInstalled, 'installed')}>Refresh Installed Apps</button>
                <button onClick={() => handleAction(fetchDownloads, 'downloads')}>Refresh Downloads</button>
                <button onClick={() => handleAction(fetchOurApps, 'ourApps')}>Refresh Our Apps</button>
            </div>
            <div>
                <input
                    type="text"
                    value={appId}
                    onChange={(e) => setAppId(e.target.value)}
                    placeholder="Enter App ID"
                />
                <button onClick={handleDownloadsForApp}>
                    Get Downloads for App
                </button>
            </div>
            <pre>{result}</pre>
        </div>
    )
}

export default Testing