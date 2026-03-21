assert ("Regular: " + 'Mixed quotes: ' + @"verbatim \n raw" + @'verbatim with ''quotes''' +
|||
    Text block line 1
    Text block line 2
|||
) == "Regular: Mixed quotes: verbatim \\n rawverbatim with 'quotes'Text block line 1\nText block line 2\n"; "Regular: Mixed quotes: verbatim \\n rawverbatim with 'quotes'Text block line 1\nText block line 2\n"
