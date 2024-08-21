import { Tooltip } from "./Tooltip";

export const DirectTooltip: React.FC = () => <Tooltip text={`A direct node publishes its own networking information on-chain: IP, port, so on. An indirect node relies on the service of routers, which are themselves direct nodes. Only register a direct node if you know what you're doing and have a public, static IP address.`}>
    <span>ⓘ</span>
</Tooltip>