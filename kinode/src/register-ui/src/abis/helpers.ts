
import { NetworkingInfo } from "../lib/types";
import { ipToBytes, portToBytes } from "../utils/kns_encoding";
import { multicallAbi, kinomapAbi, mechAbi, KINOMAP, MULTICALL } from "./";
import { encodeFunctionData, encodePacked, stringToHex, bytesToHex } from "viem";

export const generateNetworkingKeys = async ({
    direct,
    label,
    our_address,
    setNetworkingKey,
    setIpAddress,
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

    // why are we doing these? TODO
    setNetworkingKey(networking_key);
    // setIpAddress(ipAddress);
    setWsPort(ws_port || 0);
    setTcpPort(tcp_port || 0);
    setRouters(allowed_routers);

    const netkeycall = encodeFunctionData({
        abi: kinomapAbi,
        functionName: 'note',
        args: [
            encodePacked(["bytes"], [stringToHex("~net-key")]),
            encodePacked(["bytes"], [stringToHex(networking_key)]),
        ]
    });

    const ws_port_call =
        encodeFunctionData({
            abi: kinomapAbi,
            functionName: 'note',
            args: [
                encodePacked(["bytes"], [stringToHex("~ws-port")]),
                encodePacked(["bytes"], [bytesToHex(portToBytes(ws_port || 0))]),
            ]
        });

    const ip_address_call =
        encodeFunctionData({
            abi: kinomapAbi,
            functionName: 'note',
            args: [
                encodePacked(["bytes"], [stringToHex("~ip")]),
                encodePacked(["bytes"], [bytesToHex(ipAddress)]),
            ]
        });

    const router_call =
        encodeFunctionData({
            abi: kinomapAbi,
            functionName: 'note',
            args: [
                encodePacked(["bytes"], [stringToHex("~routers")]),
                encodePacked(
                    ["bytes"],
                    [stringToHex(allowed_routers.join(","))]
                )]
        });

    const calls = direct ? [
        { target: KINOMAP, callData: netkeycall },
        { target: KINOMAP, callData: ws_port_call },
        { target: KINOMAP, callData: ip_address_call },
    ] : [
        { target: KINOMAP, callData: netkeycall },
        { target: KINOMAP, callData: router_call },
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
    //     abi: kinomapAbi,
    //     functionName: 'mint',
    //     args: [
    //         our_address,
    //         encodePacked(["bytes"], [stringToHex(label)]),
    //         initCall,
    //         "0x",
    //         KINO_ACCOUNT_IMPL,
    //     ]
    // })

}
