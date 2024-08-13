import React, { useState } from 'react'
import useAppsStore from '../store'

const Testing: React.FC = () => {
    const { getDownloads, getInstalledApps, getOurApps, getDownloadsForApp } = useAppsStore()
    const [result, setResult] = useState<any>(null)
    const [appId, setAppId] = useState('')

    const handleAction = async (action: () => Promise<any>) => {
        try {
            const data = await action()
            setResult(JSON.stringify(data, null, 2))
        } catch (error) {
            setResult(`Error: ${error.message}`)
        }
    }

    return (
        <div>
            <h1>Testing Page</h1>
            <div>
                <button onClick={() => handleAction(getDownloads)}>Get Downloads</button>
                <button onClick={() => handleAction(getInstalledApps)}>Get Installed Apps</button>
                <button onClick={() => handleAction(getOurApps)}>Get Our Apps</button>
            </div>
            <div>
                <input
                    type="text"
                    value={appId}
                    onChange={(e) => setAppId(e.target.value)}
                    placeholder="Enter App ID"
                />
                <button onClick={() => handleAction(() => getDownloadsForApp(appId))}>
                    Get Downloads for App
                </button>
            </div>
            <pre>{result}</pre>
        </div>
    )
}

export default Testing