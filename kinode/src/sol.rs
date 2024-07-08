use alloy_sol_macro::sol;

sol! {
    function mint (
        address who,
        bytes calldata label,
        bytes calldata initialization,
        bytes calldata erc721Data,
        address implementation
    ) external returns (
        address tba
    );

    function get (
        bytes32 node
    ) external view returns (
        address tba,
        address owner,
        bytes data,
    );

    function note (
        bytes calldata note,
        bytes calldata data
    ) external returns (
        bytes32 notenode
    );

    function edit (
        bytes32 _note,
        bytes calldata _data
    ) external;

    // tba account
    function execute(
        address to,
        uint256 value,
        bytes calldata data,
        uint8 operation
    ) external payable returns (bytes memory returnData);

    struct Boot {
        string username;
        bytes32 password_hash;
        uint256 timestamp;
        bool direct;
        bool reset;
        uint256 chain_id;
    }

    struct Call {
        address target;
        bytes callData;
    }

    function aggregate(
        Call[] calldata calls
    ) external payable returns (uint256 blockNumber, bytes[] memory returnData);

    function token() external view returns (uint256,address,uint256);
}