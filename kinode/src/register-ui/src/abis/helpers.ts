
import { NetworkingInfo } from "../lib/types";
import { ipToNumber } from "../utils/ipToNumber";
import { multicallAbi, kinomapAbi, mechAbi, KINOMAP, MULTICALL, KINO_ACCOUNT_IMPL } from "./";
import { encodeFunctionData, encodePacked, stringToHex } from "viem";

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

    // this annoyingly fails on local development while proxying... idk why
    // so hardcoded for now 
    // const {
    //     networking_key,
    //     routing: {
    //         Both: {
    //             ip: ip_address,
    //             ports: { ws: ws_port, tcp: tcp_port },
    //             routers: allowed_routers
    //         }
    //     }
    // } = (await fetch("/generate-networking-info", { method: "POST" }).then(
    //     (res) => res.json()
    // )) as NetworkingInfo;

    const ipAddress = ipToNumber("127.0.0.1");

    setNetworkingKey(our_address);
    setIpAddress(ipAddress);
    setWsPort(9300);
    setTcpPort(9301);
    setRouters(["default-router-1.os"]);

    const netkeycall = encodeFunctionData({
        abi: kinomapAbi,
        functionName: 'note',
        args: [
            encodePacked(["bytes"], [stringToHex("~net-key")]),
            encodePacked(["bytes"], [stringToHex(our_address)]),
        ]
    });

    // TODO standardize all the KNS interactions....
    // formats of IPs across the board etc..
    const ws_port_call =
        encodeFunctionData({
            abi: kinomapAbi,
            functionName: 'note',
            args: [
                encodePacked(["bytes"], [stringToHex("~ws-port")]),
                encodePacked(["bytes"], [stringToHex("9300")]),
            ]
        });

    const ip_address_call =
        encodeFunctionData({
            abi: kinomapAbi,
            functionName: 'note',
            args: [
                encodePacked(["bytes"], [stringToHex("~ip")]),
                encodePacked(["bytes"], [stringToHex(ipAddress.toString())]),
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
                    [stringToHex("default-router-1.os")]
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
