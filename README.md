#### mpc-carrier
- 2-3 nodes that communicate with ea other, each is equal (i.e. not a backend/frontend)
- Nodes are running the same computation & talking to each other
- Protobuf on top of raw TLS TCP sockets
- Using TLS SNI
- Will eventually talk to [mpc-uniqueness-check](https://github.com/worldcoin/mpc-uniqueness-check)
