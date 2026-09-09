# Stellar IoT - Pay-Per-Use IoT Platform

A decentralized IoT platform where devices require payment via the Stellar blockchain before granting access. This project demonstrates a pay-per-use model using Stellar's Soroban smart contracts.

## Live Demo

- **App**: [stellar-io-t-web.vercel.app](https://stellar-io-t-web.vercel.app)
- **API**: [responsible-rejoicing-production-cd6a.up.railway.app](https://responsible-rejoicing-production-cd6a.up.railway.app)

## Architecture

This monorepo contains three main components:

- **Frontend (Next.js)**: User interface for browsing devices and making payments
- **Backend (Rust + Axum)**: REST API for device management and payment coordination
- **Smart Contracts (Soroban)**: On-chain payment validation and device access control

## Monorepo Structure

```
stellar-iot/
├── apps/
│   ├── web/              # Next.js frontend
│   └── api/              # Rust Axum backend
├── contracts/
│   └── iot/              # Soroban smart contracts
├── packages/
│   └── shared/           # Shared types/interfaces
├── scripts/              # Build and deployment scripts
├── .env.example          # Environment variables template
├── README.md
└── CONTRIBUTING.md
```

## Prerequisites

- Node.js 18+ and npm/yarn
- Rust 1.70+
- Stellar CLI (for Soroban contracts)
- Docker (optional, for local Stellar network)

## Setup Instructions

### 1. Clone and Install Dependencies

```bash
git clone https://github.com/yourusername/stellar-iot.git
cd stellar-iot
```

### 2. Environment Configuration

```bash
cp .env.example .env
# Edit .env with your configuration
```

### 3. Frontend Setup

```bash
cd apps/web
npm install
npm run dev
# Frontend runs on http://localhost:3000
```

### 4. Backend Setup

```bash
cd apps/api
cargo build
cargo run
# Backend runs on http://localhost:8000
```

### 5. Smart Contract Setup

```bash
cd contracts/iot
cargo build --target wasm32-unknown-unknown --release
stellar contract deploy --wasm target/wasm32-unknown-unknown/release/iot_contract.wasm --network testnet
```

## Running the Full Stack

Each component can run independently:

- **Frontend**: `cd apps/web && npm run dev`
- **Backend**: `cd apps/api && cargo run`
- **Contracts**: Deploy using Stellar CLI

## Testing

- **Frontend**: `cd apps/web && npm test`
- **Backend**: `cd apps/api && cargo test`
- **Contracts**: `cd contracts/iot && cargo test`

## How It Works

1. User browses available IoT devices on the frontend
2. User selects a device and initiates payment
3. Frontend calls backend API with payment details
4. Backend validates and calls Soroban smart contract
5. Smart contract verifies payment on Stellar network
6. Device access is granted upon successful payment

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT License - see LICENSE file for details

## Resources

- [Stellar Documentation](https://developers.stellar.org/)
- [Soroban Smart Contracts](https://soroban.stellar.org/)
- [Next.js Documentation](https://nextjs.org/docs)
- [Axum Documentation](https://docs.rs/axum/)
