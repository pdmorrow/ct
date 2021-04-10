# Crypto Trader.

Automated binance crypto trading bot written in rust using the Binance REST
API & WebSocket interface: https://binance-docs.github.io/apidocs/spot/en/#change-log

Supports the following trading strategies, some of which can be combined:

   * Moving averge cross over.
   * Moving averge trend reversal.
   * MACD.

## Configuration.

See conf/ct_template.ini for examples.

### Pairs

This is a comma separated list of trading pairs to look at, for
example:

```
Pairs=ADA/USDT
```

or

```
Pairs=ADA/USDT,BTC/USDT
```

Are both acceptable. In the case where multiple pairs are specified there
is an even distribution or capital used. So if we started with 100USDT then
in the second example we would trade ADA with 50USDT and BTC with 50USDT.

When multiple pairs are used, the configuration applies to all pairs.

#### BVLT Pairs.

This is a comma separated list of 3 pairs to look at, for example:

```
Pairs=BTC/USDT:BTCUP/USDT:BTCDOWN/USDT
```

This is called bvlt mode. What we'll do here is to perform analysis on the
first pair then trade the other 2 pairs based on the analysis of the first pair.
For example, if we think the value of BTC/USDT is going to go up, we would:

  * Sell any BTCDOWN we own.
  * Buy BTCUP.

If we think BTC/USDT is going to go down we would:

  * Sell any BTCUP we own.
  * Buy BTCDOWN.


### Time Frame

The time frame of candle stick data to look at, choose any of:

```
1m
3m
5m
15m
30m
1h
2h
4h
6h
8h
12h
1d
3d
1w
1M
```

### Slow & Fast

Slow and fast moving average values to use when using cross or trend signals.

### EMA

If set to true, use an exponetial weighted average. Otherwise use the simple
moving average.

### OrderType

#### Market

Accept whatever price is available in the market.

#### Limit

Requires the ```LimitOffset=``` option to be set to a positive integer. This is 
used to compute an acceptable limit price, the formula used is:

Buy side:

```
limit_price = close_price + (tick_size_for_symbol * LimitOffset)
```

Sell side:

```
limit_price = close_price - (tick_size_for_symbol * LimitOffset)
```

### StopPercent

Percent of movement from our purchase price we allow before triggering
a stop loss order.

Currently not supported for Leverage trading or shorting.

### Leverage

Accepts any of None, or a number between 1 & 10. Though this is coin dependent.
This makes use of Binance isolated margin and as such your account must support
this and you must have funds in your isolated margin account for the symbols you
want to trade.

Does not support stop losses!!

### Short

If set to true, enable short selling on down trends via the margin account. Your
isolated margin account must be funded.

Does not support stop losses!!

### Signals

Takes a single value, any of:

### cross

When the fast moving average crosses the slow moving average, this is an indication
to go long. The opposite is an indication to go short or to close the long position.

### trend

When a trend reversal is detected on the fast ma, buy or sell.

### macd

When the macd line crosses the signal line, buy or sell.

## Testing & Results.

TODO.

## Install.

TODO

## Logging.

TODO.

## Disclaimer.

This is a work in progress and is not fully functional currently, no liability is assumed if you use this piece of software.

## License.

MIT.
