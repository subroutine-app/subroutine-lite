package com.example.subroutine_simple.ui.creator;

import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;

// A real JNI caller, separate from the application's Kotlin bridge.
public final class NativeItemParser {
    public native String nativeParse(String requestJson);

    public static void main(String[] args) throws Exception {
        System.load(args[0]);
        NativeItemParser parser = new NativeItemParser();
        try (BufferedReader input = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8))) {
            String line;
            while ((line = input.readLine()) != null) {
                String request = switch (line) {
                    case "@null" -> null;
                    case "@surrogate" -> "\ud800";
                    case "@oversize" -> " ".repeat(131073);
                    default -> line;
                };
                System.out.println(parser.nativeParse(request));
            }
        }
    }
}
