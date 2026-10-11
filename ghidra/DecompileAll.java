import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.util.headless.HeadlessScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.address.Address;

import java.io.FileWriter;
import java.io.PrintWriter;

/**
 * Headless decompiler: dumps every function in the current program to C-like
 * pseudocode, plus an address->function index. Run as a postScript after import.
 *
 * Output goes to the path given by the first script argument.
 */
public class DecompileAll extends HeadlessScript {

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String outPath = args.length > 0 ? args[0]
                : "/home/a/projects/shogo-mad/decompiled/" + currentProgram.getName() + ".c";

        DecompInterface di = new DecompInterface();
        di.openProgram(currentProgram);

        FunctionIterator fns = currentProgram.getFunctionManager().getFunctions(true);
        int count = 0;
        try (PrintWriter out = new PrintWriter(new FileWriter(outPath))) {
            out.println("/* Decompiled by Ghidra from " + currentProgram.getName()
                    + " (Shogo: Mobile Armor Division, LithTech 1.0) */");
            out.println("/* Project: shogo-mad preservation */");
            out.println();
            for (Function f : fns) {
                Address entry = f.getEntryPoint();
                DecompileResults res = di.decompileFunction(f, 60, monitor);
                if (res.decompileCompleted() && res.getDecompiledFunction() != null) {
                    out.println("/* ===== " + f.getName() + " @ " + entry + " ===== */");
                    out.println(res.getDecompiledFunction().getC());
                    out.println();
                    count++;
                } else {
                    out.println("/* ==== " + f.getName() + " @ " + entry
                            + " : decompile failed (" + res.getErrorMessage() + ") ==== */");
                    out.println();
                }
            }
        }
        di.dispose();
        println("Decompiled " + count + " functions -> " + outPath);
    }
}
