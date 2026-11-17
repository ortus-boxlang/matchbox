<!--- CF template: <cfset> and <cfoutput> (issue #45). --->
<cfset greeting = "Hello">
<cfset name = "World">
<cfoutput>#greeting#, #name#!</cfoutput>
<cfscript>
    assert greeting & ", " & name & "!" == "Hello, World!" : "cfset values are visible in cfscript";
</cfscript>
