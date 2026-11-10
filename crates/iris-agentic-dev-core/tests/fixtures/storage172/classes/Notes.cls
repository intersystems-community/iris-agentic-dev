Class IadSm172.Notes Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Title As %String;

Property Author As %String;

Property Half As %String;

/// Library notes: one node per note, title and author as pieces.
Storage SQLStorage
{
<Description>
<![CDATA[Library notes: one node per note, title and author as pieces.]]></Description>
<SqlIdExpression>$i(^IadSm172Notes)</SqlIdExpression>
<SQLMap name="Master">
<Data name="Author">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="Half">
<RetrievalCode>
<![CDATA[ Set {*}=$Select(($Piece($Get(^IadSm172Notes({L1})),"^",1)]]]]><![CDATA["M")&1:"second",1:"first")]]></RetrievalCode>
</Data>
<Data name="Title">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Global>^IadSm172Notes</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172NotesS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

