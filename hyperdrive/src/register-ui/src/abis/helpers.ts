
import { NetworkingInfo } from "../lib/types";
import { kinohash } from "../utils/kinohash";
import { ipToBytes, portToBytes } from "../utils/hns_encoding";
import { multicallAbi, hypermapAbi, mechAbi, HYPERMAP, MULTICALL } from "./";
import { encodeFunctionData, encodePacked, stringToHex, bytesToHex } from "viem";

// Function to encode router names into keccak256 hashes
// Function to encode router names into keccak256 hashes
const encodeRouters = (routers: string[]): `0x${string}` => {
    const hashedRouters = routers.map(router => kinohash(router).slice(2)); // Remove '0x' prefix
    return `0x${hashedRouters.join('')}`;
};

export const generateNetworkingKeys = async ({
    direct,
    setNetworkingKey,
    setWsPort,
    setTcpPort,
    setRouters,
    reset,
}: {
    direct: boolean,
    label: string,
    our_address: `0x${string}`,
    setNetworkingKey: (networkingKey: string) => void;
    setIpAddress: (ipAddress: number) => void;
    setWsPort: (wsPort: number) => void;
    setTcpPort: (tcpPort: number) => void;
    setRouters: (routers: string[]) => void;
    reset: boolean;
}) => {
    const {
        networking_key,
        routing: {
            Both: {
                ip: ip_address,
                ports: { ws: ws_port, tcp: tcp_port },
                routers: allowed_routers
            }
        }
    } = (await fetch("/generate-networking-info", { method: "POST" }).then(
        (res) => res.json()
    )) as NetworkingInfo;

    const ipAddress = ipToBytes(ip_address);

    setNetworkingKey(networking_key);
    // setIpAddress(ipAddress);
    setWsPort(ws_port || 0);
    setTcpPort(tcp_port || 0);
    setRouters(allowed_routers);

    console.log("networking_key: ", networking_key);

    const netkeycall = encodeFunctionData({
        abi: hypermapAbi,
        functionName: 'note',
        args: [
            encodePacked(["bytes"], [stringToHex("~net-key")]),
            encodePacked(["bytes"], [networking_key as `0x${string}`]),
        ]
    });

    const ws_port_call =
        encodeFunctionData({
            abi: hypermapAbi,
            functionName: 'note',
            args: [
                encodePacked(["bytes"], [stringToHex("~ws-port")]),
                encodePacked(["bytes"], [bytesToHex(portToBytes(ws_port || 0))]),
            ]
        });

    const tcp_port_call =
        encodeFunctionData({
            abi: hypermapAbi,
            functionName: 'note',
            args: [
                encodePacked(["bytes"], [stringToHex("~tcp-port")]),
                encodePacked(["bytes"], [bytesToHex(portToBytes(tcp_port || 0))]),
            ]
        });

    const ip_address_call =
        encodeFunctionData({
            abi: hypermapAbi,
            functionName: 'note',
            args: [
                encodePacked(["bytes"], [stringToHex("~ip")]),
                encodePacked(["bytes"], [bytesToHex(ipAddress)]),
            ]
        });

    const encodedRouters = encodeRouters(allowed_routers);

    const router_call =
        encodeFunctionData({
            abi: hypermapAbi,
            functionName: 'note',
            args: [
                encodePacked(["bytes"], [stringToHex("~routers")]),
                encodePacked(
                    ["bytes"],
                    [encodedRouters]
                )]
        });

    const calls = direct ? [
        { target: HYPERMAP, callData: netkeycall },
        { target: HYPERMAP, callData: ws_port_call },
        { target: HYPERMAP, callData: tcp_port_call },
        { target: HYPERMAP, callData: ip_address_call },
    ] : [
        { target: HYPERMAP, callData: netkeycall },
        { target: HYPERMAP, callData: router_call },
    ];

    const multicalls = encodeFunctionData({
        abi: multicallAbi,
        functionName: 'aggregate',
        args: [calls]
    });

    if (reset) return multicalls;

    const initCall = encodeFunctionData({
        abi: mechAbi,
        functionName: 'execute',
        args: [
            MULTICALL,
            BigInt(0),
            multicalls,
            1
        ]
    });

    return initCall;

    // to mint a subname of your own, you would do something like this.
    // const mintCall = encodeFunctionData({
    //     abi: hypermapAbi,
    //     functionName: 'mint',
    //     args: [
    //         our_address,
    //         encodePacked(["bytes"], [stringToHex(label)]),
    //         initCall,
    //         KINO_ACCOUNT_IMPL,
    //     ]
    // })

}
