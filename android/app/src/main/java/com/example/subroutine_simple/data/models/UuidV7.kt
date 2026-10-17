package com.example.subroutine_simple.data.models

import java.security.SecureRandom
import java.util.UUID

object UuidV7 {
    private val random = SecureRandom()
    private var lastTimestampMillis = -1L
    private var sequence = 0

    @Synchronized
    fun newString(): String = next(System.currentTimeMillis(), random.nextLong()).toString()

    @Synchronized
    internal fun next(nowMillis: Long, randomBits: Long): UUID {
        require(nowMillis >= 0) { "UUIDv7 timestamps cannot be negative" }
        var timestamp = maxOf(nowMillis, lastTimestampMillis)
        if (timestamp == lastTimestampMillis) {
            sequence = (sequence + 1) and 0x0fff
            if (sequence == 0) timestamp += 1
        } else {
            sequence = (randomBits ushr 52).toInt() and 0x0fff
        }
        lastTimestampMillis = timestamp

        val mostSignificant = ((timestamp and 0x0000_ffff_ffff_ffffL) shl 16) or
            0x7000L or sequence.toLong()
        val leastSignificant = (randomBits and 0x3fff_ffff_ffff_ffffL) or
            Long.MIN_VALUE
        return UUID(mostSignificant, leastSignificant)
    }
}
