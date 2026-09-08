import type { Metadata } from 'next'
import { Inter } from 'next/font/google'
import './globals.css'
import Navbar from '@/components/Navbar'
import { WalletProvider } from '@/providers/WalletProvider'   // ← Add this

const inter = Inter({ subsets: ['latin'] })

export const metadata: Metadata = {
  title: 'Stellar IoT - Pay-Per-Use Devices',
  description: 'Decentralized IoT platform powered by Stellar blockchain',
}

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <html lang="en">
      <body className={inter.className}>
        <WalletProvider>                    {/* ← Wrap here */}
          <Navbar />
          <main className="min-h-screen">{children}</main>
        </WalletProvider>
      </body>
    </html>
  )
}