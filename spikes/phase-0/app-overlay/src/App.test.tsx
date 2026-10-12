import { fireEvent, render, screen } from '@testing-library/react'
import { expect, test } from 'vitest'
import App from './App'

test('renders the counter and increments it', () => {
  render(<App />)
  const button = screen.getByRole('button', { name: /count is/i })
  fireEvent.click(button)
  expect(button.textContent).toMatch(/count is 1/i)
})
