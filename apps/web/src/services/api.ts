import {
  Device,
  PaymentRequest,
  PaymentResponse,
  Session,
  DeviceRegistrationForm,
  DeviceRegistrationResponse,
  DeviceAnalyticsReport,
  ReportPeriod,
  ManagedDevice,
  DeviceUpdateForm,
  PaymentHistoryFilters,
  PaymentHistoryResponse,
  QrScanAnalytics,
  OwnerEarningsResponse,
  OwnerDeviceStatus,
  WithdrawalRequest,
  WithdrawalResponse,
} from '@/types'

const API_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:8000'

export async function getDevices(): Promise<Device[]> {
  const response = await fetch(`${API_URL}/devices`)
  if (!response.ok) throw new Error('Failed to fetch devices')
  return response.json()
}

export async function getDevice(deviceId: string): Promise<ManagedDevice> {
  const response = await fetch(`${API_URL}/devices/${deviceId}`)
  if (!response.ok) throw new Error('Failed to fetch device')
  return response.json()
}

export async function makePayment(payment: PaymentRequest): Promise<PaymentResponse> {
  const response = await fetch(`${API_URL}/pay`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payment),
  })
  if (!response.ok) throw new Error('Payment failed')
  return response.json()
}

export async function getSession(sessionId: string): Promise<Session> {
  const response = await fetch(`${API_URL}/session/${sessionId}`)
  if (!response.ok) throw new Error('Failed to fetch session')
  return response.json()
}

export async function getSessions(userAddress: string): Promise<Session[]> {
  const response = await fetch(`${API_URL}/sessions?user=${userAddress}`)
  if (!response.ok) throw new Error('Failed to fetch sessions')
  return response.json()
}

export async function extendSession(sessionId: string, payment: PaymentRequest): Promise<PaymentResponse> {
  const response = await fetch(`${API_URL}/session/${sessionId}/extend`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payment),
  })
  if (!response.ok) throw new Error('Failed to extend session')
  return response.json()
}

export async function endSession(sessionId: string): Promise<void> {
  const response = await fetch(`${API_URL}/session/${sessionId}`, { method: 'DELETE' })
  if (!response.ok) throw new Error('Failed to end session')
}

export async function registerDevice(data: DeviceRegistrationForm): Promise<DeviceRegistrationResponse> {
  const response = await fetch(`${API_URL}/devices`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  })
  if (!response.ok) throw new Error('Failed to register device')
  return response.json()
}

/**
 * Update a device. Requires the owner's Stellar address for authentication
 * (sent via the `X-Owner-Address` header).
 */
export async function updateDevice(
  deviceId: string,
  ownerAddress: string,
  data: DeviceUpdateForm,
): Promise<ManagedDevice> {
  const response = await fetch(`${API_URL}/devices/${deviceId}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json', 'X-Owner-Address': ownerAddress },
    body: JSON.stringify(data),
  })
  if (!response.ok) throw new Error('Failed to update device')
  return response.json()
}

/**
 * Deregister a device. Requires the owner's Stellar address for authentication.
 */
export async function deleteDevice(deviceId: string, ownerAddress: string): Promise<void> {
  const response = await fetch(`${API_URL}/devices/${deviceId}`, {
    method: 'DELETE',
    headers: { 'X-Owner-Address': ownerAddress },
  })
  if (!response.ok) throw new Error('Failed to delete device')
}

export function getTelemetryWsUrl(sessionId: string): string {
  const wsBase = API_URL.replace(/^http/, 'ws')
  return `${wsBase}/session/${sessionId}/telemetry`
}

export async function getDeviceAnalytics(
  deviceId: string,
  period: ReportPeriod = 'daily',
  lookback?: number,
): Promise<DeviceAnalyticsReport> {
  const params = new URLSearchParams({ period })
  if (lookback) params.set('lookback', String(lookback))
  const response = await fetch(`${API_URL}/devices/${deviceId}/analytics?${params}`)
  if (!response.ok) throw new Error('Failed to fetch analytics')
  return response.json()
}

export function getAnalyticsCsvUrl(
  deviceId: string,
  period: ReportPeriod = 'daily',
  lookback?: number,
): string {
  const params = new URLSearchParams({ period, format: 'csv' })
  if (lookback) params.set('lookback', String(lookback))
  return `${API_URL}/devices/${deviceId}/analytics?${params}`
}

// ─── Payment History ───────────────────────────────────────────────────────────

function paymentHistoryParams(user: string, filters: PaymentHistoryFilters = {}): URLSearchParams {
  const params = new URLSearchParams({ user })
  if (filters.device_id) params.set('device_id', filters.device_id)
  if (filters.status) params.set('status', filters.status)
  if (filters.from) params.set('from', filters.from)
  if (filters.to) params.set('to', filters.to)
  return params
}

export async function getPaymentHistory(
  user: string,
  filters: PaymentHistoryFilters = {},
): Promise<PaymentHistoryResponse> {
  const params = paymentHistoryParams(user, filters)
  const response = await fetch(`${API_URL}/payments?${params}`)
  if (!response.ok) throw new Error('Failed to fetch payment history')
  return response.json()
}

export function getPaymentHistoryCsvUrl(user: string, filters: PaymentHistoryFilters = {}): string {
  const params = paymentHistoryParams(user, filters)
  params.set('format', 'csv')
  return `${API_URL}/payments?${params}`
}

// ─── QR Code Scan Analytics ──────────────────────────────────────────────────────

/**
 * Records a QR-code scan for a device. Fire-and-forget: failures are swallowed
 * so analytics tracking never blocks the user's access flow.
 */
export async function recordQrScan(deviceId: string, source?: string): Promise<void> {
  const params = new URLSearchParams()
  if (source) params.set('source', source)
  const query = params.toString() ? `?${params}` : ''
  try {
    await fetch(`${API_URL}/devices/${deviceId}/qr-scan${query}`, { method: 'POST' })
  } catch {
    /* analytics is best-effort */
  }
}

export async function getQrAnalytics(deviceId: string): Promise<QrScanAnalytics> {
  const response = await fetch(`${API_URL}/devices/${deviceId}/qr-analytics`)
  if (!response.ok) throw new Error('Failed to fetch QR analytics')
  return response.json()
}

// ─── Earnings / Owner Dashboard ───────────────────────────────────────────────

export async function getOwnerEarnings(
  ownerAddress: string,
  period: ReportPeriod = 'daily',
  lookback?: number,
): Promise<OwnerEarningsResponse> {
  const params = new URLSearchParams({ owner_address: ownerAddress, period })
  if (lookback) params.set('lookback', String(lookback))
  const response = await fetch(`${API_URL}/earnings?${params}`)
  if (!response.ok) throw new Error('Failed to fetch earnings')
  return response.json()
}

export async function getOwnerDevices(ownerAddress: string): Promise<OwnerDeviceStatus[]> {
  const params = new URLSearchParams({ owner_address: ownerAddress })
  const response = await fetch(`${API_URL}/earnings/devices?${params}`)
  if (!response.ok) throw new Error('Failed to fetch owner devices')
  return response.json()
}

export async function withdrawEarnings(req: WithdrawalRequest): Promise<WithdrawalResponse> {
  const response = await fetch(`${API_URL}/earnings/withdraw`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  })
  if (!response.ok) throw new Error('Withdrawal failed')
  return response.json()
}
